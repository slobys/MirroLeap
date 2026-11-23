//! Windows native AirPlay bezel. No Tauri/WebView2 overlay, and NO SetParent.
//!
//! Changing the parent of a Tauri WebView to the independent AirPlay/SDL
//! process can reset process-wide DPI awareness (Microsoft SetParent docs).
//! That is why the previous preview enlarged the entire MirroLeap GUI and
//! still failed with "Could not enable native child-window style".
//!
//! This module uses a tiny native mouse-through layered HWND with an
//! antialiased PREMULTIPLIED BGRA iPad Air-style rim. Its whole centre is
//! transparent; it never captures, resizes or touches the video pixels.
//! The HWND owns no WebView and has no invisible CSS/non-client margins.
//! A dedicated Win32 worker follows the actual SDL client in screen pixels.
#![cfg(target_os="windows")]

use std::{
    ffi::c_void,
    ptr::{self, NonNull},
    sync::{atomic::{AtomicBool, Ordering}, Arc, Mutex, OnceLock},
    thread::{self, JoinHandle},
    time::{Duration,Instant},
};
use tauri::{AppHandle, Manager};
use crate::ios_airplay::DisplaySize;
use windows::Win32::Foundation::{HWND, POINT, RECT};
use windows::Win32::Graphics::Gdi::{
    ClientToScreen, GetMonitorInfoW, MonitorFromPoint, MonitorFromWindow, MONITORINFO,
    MONITOR_DEFAULTTONEAREST, CreateRectRgn,CreateRoundRectRgn,
    SetWindowRgn,CombineRgn,RGN_OR,DeleteObject as GdiDeleteObject,HGDIOBJ,
    GetWindowRgn,GetRgnBox,PtInRegion,
};
use windows::Win32::UI::HiDpi::{
    GetDpiForWindow,GetWindowDpiAwarenessContext,SetThreadDpiAwarenessContext,
    DPI_AWARENESS_CONTEXT,DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2,
};
use windows::Win32::UI::WindowsAndMessaging::{
    GetClientRect, GetWindowRect, IsIconic, IsWindowVisible,
    MsgWaitForMultipleObjectsEx, MWMO_INPUTAVAILABLE, QS_ALLINPUT,
};

const EX_LAYERED:u32=0x00080000;
const EX_TRANSPARENT:u32=0x00000020;
const EX_TOOLWINDOW:u32=0x00000080;
const EX_NOACTIVATE:u32=0x08000000;
const WS_POPUP:u32=0x80000000;
const SW_HIDE:i32=0;
const SW_SHOWNOACTIVATE:i32=4;
const ULW_ALPHA:u32=2;
const AC_SRC_ALPHA:u8=1;
const DIB_RGB_COLORS:u32=0;
const MAX_SIDE:i32=5200;
const MAX_PIXELS:i64=16_000_000;
const FOLLOW_MS:u32=16;
const DWMWA_WINDOW_CORNER_PREFERENCE:u32=33;
const DWMWA_BORDER_COLOR:u32=34;
const DWMWA_SYSTEMBACKDROP_TYPE:u32=38;
const DWMWCP_DONOTROUND:u32=1;
const DWMWA_COLOR_NONE:u32=0xFFFF_FFFE;
const DWMWA_COLOR_DEFAULT:u32=0xFFFF_FFFF;
const DWMSBT_NONE:u32=1;
const DWMSBT_MAINWINDOW:u32=2;
// Reuse Android's verified native bezel overscan concept: extend the opaque
// iPad lip a few physical pixels OVER the SDL video, not merely up to it.
// This removes transparent wallpaper leaks during native window movement.
const VIDEO_SEAM_COVER_LOGICAL:i32=crate::frame_layout::BEZEL_CLIP_OVERSCAN_PX+3;
// Native moves outside the toolbar drag use WinEvent notifications to keep
// the layered bezel aligned between worker polls.
const EVENT_OBJECT_LOCATIONCHANGE:u32=0x800B;
const EVENT_SYSTEM_MOVESIZEEND:u32=0x000B;
const MOVE_END_CHECK_MS:u64=80;
const OBJID_WINDOW:i32=0;
const CHILDID_SELF:i32=0;
const WINEVENT_OUTOFCONTEXT:u32=0;
const SWP_NOSIZE:u32=0x0001;
const SWP_NOZORDER:u32=0x0004;
const SWP_NOACTIVATE:u32=0x0010;
#[derive(Clone,Copy)]
struct VisibleBezel {
    video:usize,
    window:usize,
    inset:i32,
    video_width:i32,
    video_height:i32,
}

static VISIBLE_BEZELS:[Mutex<Option<VisibleBezel>>;2]=[const {Mutex::new(None)};2];
static HOOK_MOVE_ENDED:[AtomicBool;2]=[const {AtomicBool::new(false)};2];

pub(crate) fn drag_bezel(usb:bool,video:HWND)->Option<(usize,i32,i32)>{
    let target=(*VISIBLE_BEZELS[usize::from(usb)].lock().ok()?)?;
    if target.video!=video.0 as usize ||
        !unsafe{IsWindowVisible(HWND(target.window as *mut _))}.as_bool(){return None;}
    let screen=video_client(video)?;
    Some((target.window,screen.point.x-target.inset,screen.point.y-target.inset))
}

#[repr(C)]
#[derive(Clone,Copy,Default,Debug,PartialEq,Eq)]
struct Point{x:i32,y:i32}

#[derive(Clone,Copy,PartialEq,Eq,Debug)]
enum RememberPosition {
    Disabled,
    Enabled(Option<Point>),
}

impl RememberPosition {
    fn parse(value:&str)->Option<Self>{
        match value {
            "off"=>Some(Self::Disabled),
            "on"=>Some(Self::Enabled(None)),
            value=>{
                let (x,y)=value.split_once(',')?;
                let (x,y):(i32,i32)=(x.parse().ok()?,y.parse().ok()?);
                let range=-1_000_000..=1_000_000;
                (range.contains(&x) && range.contains(&y))
                    .then_some(Self::Enabled(Some(Point{x,y})))
            }
        }
    }

    fn write(self,app:&AppHandle,usb:bool)->Result<(),String>{
        let value=match self {
            Self::Disabled=>"off".into(),
            Self::Enabled(None)=>"on".into(),
            Self::Enabled(Some(p))=>format!("{},{}",p.x,p.y),
        };
        crate::ios_airplay::save_pref(app,position_pref_file(usb),&value)
    }
}

fn position_pref_file(usb:bool)->&'static str{
    if usb {"usb-window-position.txt"} else {"wifi-window-position.txt"}
}

static POSITIONS:[Mutex<RememberPosition>;2]=[const {Mutex::new(RememberPosition::Enabled(None))};2];

pub(crate) fn remember_position_status(usb:bool)->Result<bool,String>{
    POSITIONS[usize::from(usb)].lock().map(|p|*p!=RememberPosition::Disabled).map_err(|e|e.to_string())
}

pub(crate) fn toggle_remember_position(app:&AppHandle,usb:bool,hwnd:HWND)->Result<bool,String>{
    let _dpi=ThreadDpi::enter(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2)
        .ok_or("无法读取投屏窗口位置")?;
    let mut saved=POSITIONS[usize::from(usb)].lock().map_err(|e|e.to_string())?;
    let next=match *saved {
        RememberPosition::Disabled=>RememberPosition::Enabled(
            position_bounds(hwnd).map(|r|Point{x:r.left,y:r.top})),
        RememberPosition::Enabled(_)=>RememberPosition::Disabled,
    };
    next.write(app,usb)?;
    *saved=next;
    Ok(next!=RememberPosition::Disabled)
}

fn position_bounds(hwnd:HWND)->Option<RECT>{
    if unsafe{IsIconic(hwnd)}.as_bool() || is_fullscreen(hwnd){return None;}
    let mut rect=RECT::default();
    unsafe{GetWindowRect(hwnd,&mut rect)}.ok()?;
    (rect.right>rect.left && rect.bottom>rect.top).then_some(rect)
}

fn constrain_position(saved:Point,window:RECT,work:RECT)->Point{
    Point {
        x:saved.x.clamp(work.left,(work.right-(window.right-window.left)).max(work.left)),
        y:saved.y.clamp(work.top,(work.bottom-(window.bottom-window.top)).max(work.top)),
    }
}

fn restore_window_position(hwnd:HWND,window:RECT,point:Point)->bool{
    let monitor=unsafe{MonitorFromPoint(POINT{x:point.x,y:point.y},MONITOR_DEFAULTTONEAREST)};
    let mut info=MONITORINFO{cbSize:std::mem::size_of::<MONITORINFO>() as u32,..Default::default()};
    if !unsafe{GetMonitorInfoW(monitor,&mut info)}.as_bool(){return false;}
    let point=constrain_position(point,window,info.rcWork);
    (point.x!=window.left || point.y!=window.top) &&
        unsafe{SetWindowPos(hwnd.0,ptr::null_mut(),point.x,point.y,0,0,
            SWP_NOSIZE|SWP_NOZORDER|SWP_NOACTIVATE)}!=0
}

fn persist_window_position(app:&AppHandle,usb:bool,point:Point){
    if let Ok(mut saved)=POSITIONS[usize::from(usb)].lock(){
        let next=RememberPosition::Enabled(Some(point));
        if *saved!=RememberPosition::Disabled && *saved!=next {
            if let Err(error)=next.write(app,usb){
                eprintln!("[iOS window position] {error}");
            }
            *saved=next;
        }
    }
}

#[repr(C)]
#[derive(Clone,Copy)]
struct Size{cx:i32,cy:i32}
#[repr(C)]
#[derive(Clone,Copy)]
struct Blend{operation:u8,flags:u8,constant_alpha:u8,alpha_format:u8}
#[repr(C)]
#[derive(Clone,Copy)]
struct BitmapInfoHeader{
    size:u32,width:i32,height:i32,planes:u16,bits_per_pixel:u16,
    compression:u32,size_image:u32,ppm_x:i32,ppm_y:i32,
    colors_used:u32,colors_important:u32,
}
#[repr(C)]
struct BitmapInfo{header:BitmapInfoHeader,colors:[u32;1]}

#[link(name="user32")]
unsafe extern "system" {
    fn CreateWindowExW(ex:u32,class:*const u16,title:*const u16,style:u32,
        x:i32,y:i32,w:i32,h:i32,parent:*mut c_void,menu:*mut c_void,
        instance:*mut c_void,extra:*mut c_void)->*mut c_void;
    fn DestroyWindow(hwnd:*mut c_void)->i32;
    fn ShowWindow(hwnd:*mut c_void,command:i32)->i32;
    fn GetDC(hwnd:*mut c_void)->*mut c_void;
    fn ReleaseDC(hwnd:*mut c_void,dc:*mut c_void)->i32;
    fn UpdateLayeredWindow(hwnd:*mut c_void,screen:*mut c_void,
        dest:*const Point,size:*const Size,surface:*mut c_void,
        src:*const Point,key:u32,blend:*const Blend,flags:u32)->i32;
    fn PeekMessageW(msg:*mut WinMsg,hwnd:*mut c_void,min:u32,max:u32,remove:u32)->i32;
    fn TranslateMessage(msg:*const WinMsg)->i32;
    fn DispatchMessageW(msg:*const WinMsg)->isize;
    fn SetWindowPos(hwnd:*mut c_void,insert_after:*mut c_void,
        x:i32,y:i32,width:i32,height:i32,flags:u32)->i32;
    fn SetWinEventHook(min_event:u32,max_event:u32,module:*mut c_void,
        callback:Option<unsafe extern "system" fn(*mut c_void,u32,*mut c_void,i32,i32,u32,u32)>,
        process_id:u32,thread_id:u32,flags:u32)->*mut c_void;
    fn UnhookWinEvent(hook:*mut c_void)->i32;
}
#[link(name="gdi32")]
unsafe extern "system" {
    fn CreateCompatibleDC(dc:*mut c_void)->*mut c_void;
    fn DeleteDC(dc:*mut c_void)->i32;
    fn CreateDIBSection(dc:*mut c_void,info:*const BitmapInfo,usage:u32,
        bits:*mut *mut c_void,section:*mut c_void,offset:u32)->*mut c_void;
    fn SelectObject(dc:*mut c_void,object:*mut c_void)->*mut c_void;
    fn DeleteObject(object:*mut c_void)->i32;
}
#[repr(C)]
#[derive(Clone,Copy)]
struct WinMsg{
    hwnd:*mut c_void,message:u32,wparam:usize,lparam:isize,time:u32,
    pt:Point,private:u32,
}
impl Default for WinMsg{
    fn default()->Self{unsafe{std::mem::zeroed()}}
}

#[link(name="dwmapi")]
unsafe extern "system" {
    fn DwmGetWindowAttribute(hwnd:*mut c_void,attribute:u32,
        out:*mut c_void,bytes:u32)->i32;
    fn DwmSetWindowAttribute(hwnd:*mut c_void,attribute:u32,
        value:*const c_void,bytes:u32)->i32;
}
fn dwm_read_u32(hwnd:HWND,attribute:u32)->Option<u32>{
    let mut value=0u32;
    let result=unsafe{DwmGetWindowAttribute(hwnd.0,attribute,
        (&mut value as *mut u32).cast(),4)};
    (result>=0).then_some(value)
}
fn dwm_write_u32(hwnd:HWND,attribute:u32,value:u32)->bool{
    (unsafe{DwmSetWindowAttribute(hwnd.0,attribute,
        (&value as *const u32).cast(),4)})>=0
}
/// Receiver-scoped DWM override. Restored when MirroLeap stops mirroring;
/// unlike SetParent, this never changes MirroLeap's process DPI awareness.
struct ReceiverDwmStyle{
    hwnd:usize,
    old_border:Option<u32>,
    old_corner:Option<u32>,
    old_backdrop:Option<u32>,
    wrote_border:bool,
    wrote_corner:bool,
}
impl ReceiverDwmStyle {
    fn attach(hwnd:HWND)->Self{
        let old_border=dwm_read_u32(hwnd,DWMWA_BORDER_COLOR);
        let old_corner=dwm_read_u32(hwnd,DWMWA_WINDOW_CORNER_PREFERENCE);
        let wrote_border=dwm_write_u32(hwnd,DWMWA_BORDER_COLOR,DWMWA_COLOR_NONE);
        let wrote_corner=dwm_write_u32(hwnd,DWMWA_WINDOW_CORNER_PREFERENCE,
            DWMWCP_DONOTROUND);
        // AirPlay-Windows v0.4.1 explicitly enables Mica behind its SDL
        // window. Do not let that material show through the rounded stage.
        let backdrop=dwm_read_u32(hwnd,DWMWA_SYSTEMBACKDROP_TYPE);
        let old_backdrop=dwm_write_u32(hwnd,DWMWA_SYSTEMBACKDROP_TYPE,DWMSBT_NONE)
            .then_some(backdrop.unwrap_or(DWMSBT_MAINWINDOW));
        Self{hwnd:hwnd.0 as usize,old_border,old_corner,old_backdrop,wrote_border,wrote_corner}
    }
    fn refresh(&self,hwnd:HWND){
        if self.wrote_border && dwm_read_u32(hwnd,DWMWA_BORDER_COLOR)
            .is_some_and(|color|color!=DWMWA_COLOR_NONE){
            let _=dwm_write_u32(hwnd,DWMWA_BORDER_COLOR,DWMWA_COLOR_NONE);
        }
        if self.old_backdrop.is_some() && dwm_read_u32(hwnd,DWMWA_SYSTEMBACKDROP_TYPE)
            .is_some_and(|style|style!=DWMSBT_NONE){
            let _=dwm_write_u32(hwnd,DWMWA_SYSTEMBACKDROP_TYPE,DWMSBT_NONE);
        }
    }
    fn restore(self){
        let hwnd=HWND(self.hwnd as *mut _);
        if self.wrote_border {
            let _=dwm_write_u32(hwnd,DWMWA_BORDER_COLOR,
                self.old_border.unwrap_or(DWMWA_COLOR_DEFAULT));
        }
        if self.wrote_corner{
            let _=dwm_write_u32(hwnd,DWMWA_WINDOW_CORNER_PREFERENCE,
                self.old_corner.unwrap_or(0));
        }
        if let Some(backdrop)=self.old_backdrop{
            let _=dwm_write_u32(hwnd,DWMWA_SYSTEMBACKDROP_TYPE,backdrop);
        }
    }
}

#[derive(Clone,Copy,Debug,PartialEq,Eq)]
struct VideoClient{point:Point,width:i32,height:i32}
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
struct CaseLayout{point:Point,width:i32,height:i32,ipad:bool,dpi:u32}

fn video_client(hwnd:HWND)->Option<VideoClient>{
    if unsafe{IsIconic(hwnd)}.as_bool() ||
        !unsafe{IsWindowVisible(hwnd)}.as_bool(){return None;}
    client_rect(hwnd)
}

fn client_rect(hwnd:HWND)->Option<VideoClient>{
    let mut rect=RECT::default();
    unsafe{GetClientRect(hwnd,&mut rect)}.ok()?;
    let mut origin=POINT::default();
    if !unsafe{ClientToScreen(hwnd,&mut origin)}.as_bool(){return None;}
    let width=rect.right-rect.left;
    let height=rect.bottom-rect.top;
    if !(160..=MAX_SIDE).contains(&width) ||
        !(120..=MAX_SIDE).contains(&height){return None;}
    Some(VideoClient{point:Point{x:origin.x,y:origin.y},width,height})
}

pub(crate) fn receiver_client_size(pid:u32)->Option<(u32,u32)>{
    let hwnd=crate::commands::find_window_by_pid(pid)?;
    let video=video_client(hwnd)?;
    Some((video.width as u32,video.height as u32))
}

pub(crate) fn is_fullscreen(hwnd:HWND)->bool{
    let monitor=unsafe{MonitorFromWindow(hwnd,MONITOR_DEFAULTTONEAREST)};
    if monitor.0.is_null(){return false;}
    let mut info=MONITORINFO{
        cbSize:std::mem::size_of::<MONITORINFO>() as u32,
        ..Default::default()
    };
    if !unsafe{GetMonitorInfoW(monitor,&mut info)}.as_bool(){return false;}
    let mut rect=RECT::default();
    if unsafe{GetWindowRect(hwnd,&mut rect)}.is_err(){return false;}
    let screen=info.rcMonitor;
    rect.left<=screen.left+4 && rect.top<=screen.top+4
        && rect.right>=screen.right-4 && rect.bottom>=screen.bottom-4
}

fn choose_ipad(style:&str,width:i32,height:i32)->bool{
    match style {
        "device-ipad"=>true,
        "device-iphone"=>false,
        _=>width.min(height) as f64 / width.max(height) as f64 >=0.62,
    }
}
fn source_size(source:Option<(u32,u32)>,display:DisplaySize,scale:f64,work:(i32,i32))->(i32,i32){
    let (w,h)=source.unwrap_or(crate::ios_airplay::WAITING_PHONE_SIZE);
    let fit=if source.is_none(){
        scale.min((f64::from((work.0-80).max(160))/f64::from(w))
            .min(f64::from((work.1-80).max(120))/f64::from(h)))
    }else{
        // USB has always used 85% of the shorter desktop dimension minus
        // its 100px clearance. Use the same baseline in both transports.
        let limit=f64::from((work.0.min(work.1)-100).max(120));
        let longest=f64::from(w.max(h));
        match display{
            DisplaySize::Small=>0.62*limit/longest,
            DisplaySize::Adaptive=>0.85*limit/longest,
            DisplaySize::Medium=>0.90*limit/longest,
            DisplaySize::Large=>0.97*limit/longest,
            DisplaySize::Source=>(limit/longest).min(1.0),
        }
    };
    let pixels=|dimension| {
        let scaled=f64::from(dimension)*fit;
        if display==DisplaySize::Adaptive && source.is_some(){scaled.floor().max(1.0) as i32}
        else{scaled.round().max(1.0) as i32}
    };
    (pixels(w),pixels(h))
}

fn desired_video_size(hwnd:HWND,outline:RegionScale,source:Option<(u32,u32)>,display:DisplaySize)->(i32,i32){
    let work=crate::device_frame::monitor_work_area(hwnd);
    let dpi_scale=f64::from(unsafe{GetDpiForWindow(hwnd)}.max(96))/96.0;
    let scale=dpi_scale.max(f64::from(outline.physical.0)/f64::from(outline.receiver.0));
    source_size(source,display,scale,
        (work.width,work.height))
}

fn resize_for_source(hwnd:HWND,screen:VideoClient,source:Option<(u32,u32)>,display:DisplaySize)->bool{
    let mut outer=RECT::default();
    if unsafe{GetWindowRect(hwnd,&mut outer)}.is_err(){return false;}
    let Some(scale)=RegionScale::for_window(hwnd,outer) else{return false;};
    let work=crate::device_frame::monitor_work_area(hwnd);
    let (width,height)=desired_video_size(hwnd,scale,source,display);
    if (screen.width-width).abs()<=2 && (screen.height-height).abs()<=2{return true;}
    let outer_width=width+scale.physical.0-screen.width;
    let outer_height=height+scale.physical.1-screen.height;
    let fit_position=|origin:i32,current:i32,target:i32,start:i32,span:i32|{
        (origin+(current-target)/2).clamp(start+20,(start+span-target-20).max(start+20))
    };
    let x=fit_position(outer.left,scale.physical.0,outer_width,work.x,work.width);
    let y=fit_position(outer.top,scale.physical.1,outer_height,work.y,work.height);
    (unsafe{SetWindowPos(hwnd.0,ptr::null_mut(),x,y,outer_width,outer_height,
        SWP_NOZORDER|SWP_NOACTIVATE)})!=0
}

pub(crate) fn prepare_background_window(pid:u32,compact:bool)->Result<usize,String>{
    use windows::Win32::Foundation::LPARAM;
    use windows::Win32::UI::WindowsAndMessaging::{EnumWindows,GetClassNameW,GetWindowThreadProcessId};
    struct Lookup{pid:u32,window:HWND}
    unsafe extern "system" fn find_sdl(hwnd:HWND,data:LPARAM)->windows::core::BOOL{
        let search=unsafe{&mut *(data.0 as *mut Lookup)};
        let mut pid=0;
        unsafe{GetWindowThreadProcessId(hwnd,Some(&mut pid));}
        if pid==search.pid{
            let mut class=[0u16;16];
            let len=unsafe{GetClassNameW(hwnd,&mut class)}.max(0) as usize;
            if String::from_utf16_lossy(&class[..len])=="SDL_app"{
                search.window=hwnd;
                return windows::core::BOOL(0);
            }
        }
        windows::core::BOOL(1)
    }
    let _dpi=ThreadDpi::enter(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2)
        .ok_or("无法初始化 iOS 窗口 DPI")?;
    let deadline=Instant::now()+Duration::from_secs(4);
    let mut settle_after=None;
    while Instant::now()<deadline{
        let mut lookup=Lookup{pid,window:HWND::default()};
        let _=unsafe{EnumWindows(Some(find_sdl),LPARAM(&mut lookup as *mut _ as isize))};
        let hwnd=lookup.window;
        if !hwnd.0.is_null(){
            // SDL applies its initial minimum size during startup. Keep the
            // window hidden until it accepts and reports the compact bounds.
            if unsafe{IsWindowVisible(hwnd)}.as_bool(){unsafe{ShowWindow(hwnd.0,SW_HIDE);}}
            let settle_after=*settle_after.get_or_insert_with(||Instant::now()+Duration::from_millis(180));
            if Instant::now()>=settle_after{
                if !compact{return Ok(hwnd.0 as usize);}
                let mut outer=RECT::default();
                let outline=unsafe{GetWindowRect(hwnd,&mut outer)}.ok()
                    .and_then(|()|RegionScale::for_window(hwnd,outer));
                if let Some((screen,outline))=client_rect(hwnd).zip(outline){
                    let display=DisplaySize::default();
                    let wanted=desired_video_size(hwnd,outline,None,display);
                    if (screen.width-wanted.0).abs()<=2 && (screen.height-wanted.1).abs()<=2{
                        return Ok(hwnd.0 as usize);
                    }
                    resize_for_source(hwnd,screen,None,display);
                }
            }
        }
        thread::sleep(Duration::from_millis(5));
    }
    Err("AirPlay 接收器未能准备好后台窗口。请关闭其他 AirPlay 接收器后重试。".into())
}

fn managed_receiver_window(state:&crate::ios_airplay::IosAirplayState,pid:u32)->Option<HWND>{
    let (owner,handle)=(*state.managed_window.lock().ok()?)?;
    if owner!=pid{return None;}
    let hwnd=HWND(handle as *mut _);
    let mut actual=0;
    unsafe{windows::Win32::UI::WindowsAndMessaging::GetWindowThreadProcessId(hwnd,Some(&mut actual));}
    (actual==pid).then_some(hwnd)
}

fn show_live_receiver(state:&crate::ios_airplay::IosAirplayState,pid:u32,video:HWND)->bool{
    if managed_receiver_window(state,pid)!=Some(video){return false;}
    let Ok(source)=state.source_pixels.lock() else{return false;};
    if source.is_none(){return false;}
    unsafe{ShowWindow(video.0,SW_SHOWNOACTIVATE);}
    unsafe{IsWindowVisible(video)}.as_bool()
}
fn bezel_inset(ipad:bool,dpi:u32)->i32{
    let logical=if ipad{12}else{18};
    ((logical as i64 * i64::from(dpi.clamp(96,384))+48)/96)
        .clamp(10,72) as i32
}
fn seam_cover_px(dpi:u32)->i32 {
    ((VIDEO_SEAM_COVER_LOGICAL as i64*i64::from(dpi.clamp(96,384))+48)/96)
        .clamp(4,16) as i32
}
/// The approved inner and outer bezel curves are concentric. The video
/// extends underneath the full opaque lip to absorb GDI/DPI raster rounding.
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
struct BezelGeometry{
    inset:i32,
    cover:i32,
    outer_radius:i32,
    inner_radius:i32,
}
impl BezelGeometry {
    fn new(ipad:bool,dpi:u32)->Self{
        let inset=bezel_inset(ipad,dpi);
        let cover=seam_cover_px(dpi);
        let scale=dpi.clamp(96,384) as f64/96.0;
        let inner_radius=((if ipad {16.0}else{44.0})*scale).round() as i32;
        Self{
            inset,cover,
            outer_radius:inner_radius+inset+cover,
            inner_radius,
        }
    }
    fn aperture_inset(self)->i32{self.inset+self.cover}
    fn video_clip_radius(self)->i32{self.inner_radius+self.cover}
}
fn target_layout(video:VideoClient,style:&str,dpi:u32)->Option<CaseLayout>{
    let ipad=choose_ipad(style,video.width,video.height);
    let inset=bezel_inset(ipad,dpi);
    let width=video.width.checked_add(inset*2)?;
    let height=video.height.checked_add(inset*2)?;
    if width>MAX_SIDE || height>MAX_SIDE ||
        i64::from(width)*i64::from(height)>MAX_PIXELS{return None;}
    Some(CaseLayout{
        point:Point{
            x:video.point.x.checked_sub(inset)?,
            y:video.point.y.checked_sub(inset)?,
        },
        width,height,ipad,dpi,
    })
}

pub(crate) fn tools_aperture(hwnd:HWND,style:&str,source:Option<(u32,u32)>)
    ->Option<(crate::frame_layout::ScreenRect,crate::frame_layout::FrameRect,i32)>
{
    use crate::frame_layout::{ScreenRect,FrameRect};
    let video=video_client(hwnd)?;
    let dpi=unsafe{GetDpiForWindow(hwnd)}.clamp(96,384);
    let case_style=match (style,source){
        ("device-auto",Some((w,h))) if choose_ipad(style,w as i32,h as i32)=>"device-ipad",
        ("device-auto",_)=>"device-iphone",
        _=>style,
    };
    let layout=target_layout(video,case_style,dpi)?;
    let geometry=BezelGeometry::new(layout.ipad,dpi);
    Some((ScreenRect{x:video.point.x+geometry.cover,y:video.point.y+geometry.cover,
        width:video.width-2*geometry.cover,height:video.height-2*geometry.cover},
        FrameRect{x:layout.point.x,y:layout.point.y,width:layout.width,height:layout.height,
            inset_x:geometry.aperture_inset(),inset_y:geometry.aperture_inset()},
        geometry.inner_radius))
}

#[derive(Clone,Copy,Debug,PartialEq,Eq)]
struct NativeVideoClip{
    hwnd:usize,
    width:i32,
    height:i32,
    local_left:i32,
    local_top:i32,
    ipad:bool,
    dpi:u32,
    scale:RegionScale,
}
pub(crate) struct ThreadDpi(DPI_AWARENESS_CONTEXT);
impl ThreadDpi{
    pub(crate) fn physical()->Option<Self>{
        Self::enter(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2)
    }
    fn enter(context:DPI_AWARENESS_CONTEXT)->Option<Self>{
        let previous=unsafe{SetThreadDpiAwarenessContext(context)};
        (!previous.0.is_null()).then_some(Self(previous))
    }
}
impl Drop for ThreadDpi{
    fn drop(&mut self){unsafe{SetThreadDpiAwarenessContext(self.0);}}
}

/// SetWindowRgn consumes coordinates in the receiver's DPI units. The bezel
/// worker reads physical pixels, so a DPI-unaware receiver at 150% would get
/// a region 1.5× too large if we passed those pixels through unchanged.
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
struct RegionScale{
    physical:(i32,i32),
    receiver:(i32,i32),
}
impl RegionScale{
    fn for_window(hwnd:HWND,physical:RECT)->Option<Self>{
        let _receiver_dpi=ThreadDpi::enter(unsafe{GetWindowDpiAwarenessContext(hwnd)})?;
        let mut logical=RECT::default();
        unsafe{GetWindowRect(hwnd,&mut logical)}.ok()?;
        let physical=(physical.right-physical.left,physical.bottom-physical.top);
        let receiver=(logical.right-logical.left,logical.bottom-logical.top);
        (physical.0>0 && physical.1>0 && receiver.0>0 && receiver.1>0)
            .then_some(Self{physical,receiver})
    }
    fn rect(self,left:i32,top:i32,right:i32,bottom:i32)->RECT{
        let floor=|value:i32,to:i32,from:i32| (value*to).div_euclid(from);
        let ceil=|value:i32,to:i32,from:i32| -floor(-value,to,from);
        RECT{
            left:floor(left,self.receiver.0,self.physical.0),
            top:floor(top,self.receiver.1,self.physical.1),
            right:ceil(right,self.receiver.0,self.physical.0),
            bottom:ceil(bottom,self.receiver.1,self.physical.1),
        }
    }
    fn diameter(self,radius:i32)->(i32,i32){
        // Round the ellipse down so discretization adds video behind the lip.
        let diameter=2*radius;
        ((diameter*self.receiver.0/self.physical.0).max(2),
         (diameter*self.receiver.1/self.physical.1).max(2))
    }
}
fn clip_signature(hwnd:HWND,client:VideoClient,ipad:bool,dpi:u32)
    ->Option<NativeVideoClip>
{
    let mut outer=RECT::default();
    unsafe{GetWindowRect(hwnd,&mut outer)}.ok()?;
    let scale=RegionScale::for_window(hwnd,outer)?;
    if scale.physical.0>MAX_SIDE || scale.physical.1>MAX_SIDE{return None;}
    Some(NativeVideoClip{
        hwnd:hwnd.0 as usize,width:client.width,height:client.height,
        local_left:client.point.x.checked_sub(outer.left)?,
        local_top:client.point.y.checked_sub(outer.top)?,
        ipad,dpi,scale,
    })
}
fn clip_shape(client:VideoClient,outer:RECT,ipad:bool,dpi:u32)
    ->Option<(i32,i32,i32,i32,i32)>
{
    let geometry=BezelGeometry::new(ipad,dpi);
    let left=client.point.x.checked_sub(outer.left)?;
    let top=client.point.y.checked_sub(outer.top)?;
    let right=left.checked_add(client.width)?;
    let bottom=top.checked_add(client.height)?;
    let radius=geometry.video_clip_radius();
    (right>left+2*radius && bottom>top+2*radius)
        .then_some((left,top,right,bottom,radius))
}
/// Restore SDL borderless window resizing ONLY along straight bezel sides.
/// Preserve no rectangular native corner (that was the white arc).
/// These narrow HWND strips remain beneath the fully opaque BGRA shell.
fn add_straight_resize_hits(
    aperture:windows::Win32::Graphics::Gdi::HRGN,
    client:VideoClient,outer:RECT,ipad:bool,dpi:u32,
    shape:(i32,i32,i32,i32,i32),
    scale:RegionScale,
){
    let (left,top,right,bottom,r)=shape;
    let inset=bezel_inset(ipad,dpi);
    let w=outer.right-outer.left;
    let h=outer.bottom-outer.top;
    let cx=client.point.x-outer.left;
    let cy=client.point.y-outer.top;
    let left_edge=(cx-inset+2).max(0);
    let right_edge=(cx+client.width+inset-2).min(w);
    let top_edge=(cy-inset+2).max(0);
    let bottom_edge=(cy+client.height+inset-2).min(h);
    let x1=left+r+2;
    let x2=right-r-2;
    let y1=top+r+2;
    let y2=bottom-r-2;
    let strips=[
        (left_edge,y1,left+1,y2),
        (right-1,y1,right_edge,y2),
        (x1,top_edge,x2,top+1),
        (x1,bottom-1,x2,bottom_edge),
    ];
    for (a,b,c,d) in strips {
        if c<=a || d<=b {continue;}
        let rect=scale.rect(a,b,c,d);
        let strip=unsafe{CreateRectRgn(rect.left,rect.top,rect.right,rect.bottom)};
        if strip.0.is_null(){continue;}
        let _=unsafe{CombineRgn(Some(aperture),Some(aperture),Some(strip),RGN_OR)};
        let _=unsafe{GdiDeleteObject(HGDIOBJ(strip.0))};
    }
}
/// Clip the receiver's video and keep the straight-side resize hit strips.
fn apply_video_corner_clip(hwnd:HWND,client:VideoClient,clip:NativeVideoClip)
    ->Result<(),&'static str>
{
    let bounds=clip.physical_bounds(client);
    let (left,top,right,bottom,r)=clip_shape(client,bounds,clip.ipad,clip.dpi)
        .ok_or("Invalid rounded video aperture")?;
    let rect=clip.scale.rect(left,top,right+1,bottom+1);
    let (diameter_x,diameter_y)=clip.scale.diameter(r);
    let aperture=unsafe{CreateRoundRectRgn(
        rect.left,rect.top,rect.right,rect.bottom,diameter_x,diameter_y)};
    if aperture.0.is_null(){return Err("Cannot create AirPlay rounded aperture");}
    add_straight_resize_hits(aperture,client,bounds,clip.ipad,clip.dpi,
        (left,top,right,bottom,r),clip.scale);
    if unsafe{SetWindowRgn(hwnd,Some(aperture),true)}==0 {
        let _=unsafe{GdiDeleteObject(HGDIOBJ(aperture.0))};
        return Err("Windows rejected the video rounded clip");
    }
    Ok(())
}
impl NativeVideoClip{
    fn physical_bounds(self,client:VideoClient)->RECT{
        let left=client.point.x-self.local_left;
        let top=client.point.y-self.local_top;
        RECT{left,top,right:left+self.scale.physical.0,bottom:top+self.scale.physical.1}
    }
}
fn native_clip_intact(hwnd:HWND,client:VideoClient,clip:NativeVideoClip)->bool{
    let bounds=clip.physical_bounds(client);
    let Some((left,top,right,bottom,r))=
        clip_shape(client,bounds,clip.ipad,clip.dpi) else{return false};
    let rgn=unsafe{CreateRectRgn(0,0,0,0)};
    if rgn.0.is_null(){return false;}
    let kind=unsafe{GetWindowRgn(hwnd,rgn)}.0;
    let mut actual=RECT::default();
    let corner=(r/6).max(2);
    let corners=[(left+corner,top+corner),(right-1-corner,top+corner),
        (left+corner,bottom-1-corner),(right-1-corner,bottom-1-corner)];
    let valid=kind>=2 && unsafe{GetRgnBox(rgn,&mut actual)}.0>=2
        && (actual.right-1-right).abs()<=bezel_inset(clip.ipad,clip.dpi)+4
        && (actual.bottom-1-bottom).abs()<=bezel_inset(clip.ipad,clip.dpi)+4
        && corners.into_iter().all(|(x,y)| !unsafe{PtInRegion(rgn,x,y)}.as_bool())
        && unsafe{PtInRegion(rgn,(left+right)/2,(top+bottom)/2)}.as_bool();
    let _=unsafe{GdiDeleteObject(HGDIOBJ(rgn.0))};
    valid
}
fn clear_video_corner_clip(hwnd:HWND){
    let _=unsafe{SetWindowRgn(hwnd,None,true)};
}
fn clear_clip_if_still_owned(pid:u32,applied:&mut Option<NativeVideoClip>){
    if let Some(previous)=applied.take() {
        if let Some(video)=crate::commands::find_window_by_pid(pid){
            if video.0 as usize==previous.hwnd{
                clear_video_corner_clip(video);
            }
        }
    }
}

// Exact signed distance to a rounded rectangle in physical pixels:
// negative inside, zero at the metal contour, positive outside.
fn round_signed_distance(px:f32,py:f32,left:f32,top:f32,w:f32,h:f32,r:f32)->f32 {
    let r=r.max(1.0).min(w.min(h)*0.5);
    let dx=(px-(left+w*0.5)).abs()-(w*0.5-r);
    let dy=(py-(top+h*0.5)).abs()-(h*0.5-r);
    let outside=(dx.max(0.0)*dx.max(0.0)+dy.max(0.0)*dy.max(0.0)).sqrt();
    outside+dx.max(dy).min(0.0)-r
}
fn round_coverage(px:f32,py:f32,left:f32,top:f32,w:f32,h:f32,r:f32)->f32 {
    (0.5-round_signed_distance(px,py,left,top,w,h,r)).clamp(0.0,1.0)
}

fn antialiased_coverage(x:f32,y:f32,distance:f32,sdf:impl Fn(f32,f32)->f32)->f32{
    if distance<=-0.75{return 1.0;}
    if distance>=0.75{return 0.0;}
    let mut covered=0.0;
    for dy in [-0.375,-0.125,0.125,0.375]{
        for dx in [-0.375,-0.125,0.125,0.375]{
            covered+=(0.5-4.0*sdf(x+dx,y+dy)).clamp(0.0,1.0);
        }
    }
    covered/16.0
}

fn metal_band(distance:f32,scale:f32,shine:f32)->(f32,f32,f32) {
    // Distance is measured INWARD from the curved metal contour, not from
    // the rectangular canvas edge. A ~4px antialiased silver strip follows
    // every quarter-circle instead of disappearing into graphite at corners.
    let layers=[(2.1*scale,(220.0+19.0*shine,225.0+18.0*shine,230.0+17.0*shine)),
        (3.15*scale,(118.0+36.0*shine,133.0+32.0*shine,147.0+29.0*shine)),
        (5.4*scale,(49.0+12.0*shine,57.0+10.0*shine,67.0+8.0*shine)),
        (8.0*scale,(21.0,26.0,31.0))];
    let mut color=(9.0+3.0*shine,12.0+3.0*shine,17.0+3.0*shine);
    for (edge,outer) in layers.into_iter().rev(){
        // The internal metal/graphite transitions need AA as much as the
        // transparent silhouette, otherwise they show a staircase at 150%.
        let weight=(edge+0.5-distance).clamp(0.0,1.0);
        color=(color.0+(outer.0-color.0)*weight,
            color.1+(outer.1-color.1)*weight,color.2+(outer.2-color.2)*weight);
    }
    color
}

/// Build alpha-premultiplied silver/graphite ring in pure code. The middle is
/// COMPLETELY transparent (zero BGRA), so no white/opaque WebView rectangles.
fn render_bezel(layout:CaseLayout,camera_right:bool)->Result<Vec<u8>,&'static str>{
    let w=layout.width;
    let h=layout.height;
    if w<=0 || h<=0 || i64::from(w)*i64::from(h)>MAX_PIXELS {
        return Err("Invalid native bezel surface");
    }
    let dpi=layout.dpi.clamp(96,384);
    let scale=dpi as f32/96.0;
    let geometry=BezelGeometry::new(layout.ipad,dpi);
    let inner_inset=geometry.aperture_inset();
    let radius=geometry.outer_radius as f32;
    let hole_radius=geometry.inner_radius as f32;
    // The actual contour is tangent to each DIB edge. Moving it outside
    // the bitmap cuts the curve short at eight separate intersections.
    let mut pixels=vec![0u8;(w as usize)*(h as usize)*4];
    let safe_corner_x=inner_inset+hole_radius.ceil() as i32+2;
    let safe_corner_y=safe_corner_x;
    for y in 0..h {
        let yf=y as f32+0.5;
        let inside_y=y>=inner_inset+2 && y<h-inner_inset-2;
        for x in 0..w {
            // Old optimization skipped the ENTIRE rectangular centre,
            // including all 4 rounded-hole corners. Those video corners
            // remained square and looked like broken/cut device rims.
            // Skip ONLY the provably transparent straight-edge interior.
            if inside_y && x>=inner_inset+2 && x<w-inner_inset-2
                && ((x>=safe_corner_x && x<w-safe_corner_x)
                    || (y>=safe_corner_y && y<h-safe_corner_y)){
                continue;
            }
            let xf=x as f32+0.5;
            let outer_shape=|x,y|round_signed_distance(
                x,y,0.0,0.0,w as f32,h as f32,radius);
            let outer_distance=outer_shape(xf,yf);
            let outer=antialiased_coverage(xf,yf,outer_distance,outer_shape);
            if outer==0.0{continue;}
            let inner_shape=|x,y|round_signed_distance(x,y,inner_inset as f32,inner_inset as f32,
                (w-2*inner_inset) as f32,(h-2*inner_inset) as f32,hole_radius);
            let inner=antialiased_coverage(xf,yf,inner_shape(xf,yf),inner_shape);
            let opacity=(outer*(1.0-inner)).clamp(0.0,1.0);
            if opacity==0.0{continue;}
            // A bright silver crest over the darker bevel gives the metal
            // a defined edge while the inner glass remains matte.
            // The metal band follows the curved distance field, not the
            // rectangle's min(x,y,w-x,h-y) which made every corner go black.
            let shine=((w as f32-xf)/w as f32*0.10)+
                ((h as f32-yf)/h as f32*0.035);
            let distance_inside=(-outer_distance).max(0.0)+1.1*scale;
            let (r,g,b)=if distance_inside>=inner_inset as f32-1.7*scale{
                (17.0,22.0,28.0)
            }else{metal_band(distance_inside,scale,shine)};
            let a=(opacity*255.0).round() as u8;
            let pos=((y as usize)*(w as usize)+x as usize)*4;
            pixels[pos]=(b*a as f32/255.0).round() as u8;
            pixels[pos+1]=(g*a as f32/255.0).round() as u8;
            pixels[pos+2]=(r*a as f32/255.0).round() as u8;
            pixels[pos+3]=a;
        }
    }
    // Small iPad Air front camera on the rim, on long or short edge
    // depending on the actual orientation, never in the video aperture.
    let center=camera_center(layout,camera_right);
    let cr=if layout.ipad{2.8*scale}else{3.2*scale};
    let area=(6.0*scale).ceil() as i32;
    for y in (center.1 as i32-area).max(0)..(center.1 as i32+area).min(h){
        for x in (center.0 as i32-area).max(0)..(center.0 as i32+area).min(w){
            let distance=((x as f32+0.5-center.0).powi(2)+
                (y as f32+0.5-center.1).powi(2)).sqrt();
            let coverage=(cr+0.5-distance).clamp(0.0,1.0);
            if coverage==0.0{continue;}
            let pos=((y as usize)*(w as usize)+x as usize)*4;
            let shade=if distance<cr*0.57{[55.0,42.0,24.0]}else{[12.0,7.0,4.0]};
            for (component,color) in pixels[pos..pos+4].iter_mut()
                .zip(shade.into_iter().chain(std::iter::once(255.0))){
                *component=(color*coverage+f32::from(*component)*(1.0-coverage)).round() as u8;
            }
        }
    }
    Ok(pixels)
}

fn camera_center(layout:CaseLayout,right:bool)->(f32,f32){
    let w=layout.width as f32;
    let h=layout.height as f32;
    let inset=7.0*layout.dpi.clamp(96,384) as f32/96.0;
    if w>h{
        (if right{w-inset}else{inset},h*0.5)
    }else{
        (w*0.5,inset)
    }
}

struct Surface {
    dc:NonNull<c_void>,
    bitmap:Option<NonNull<c_void>>,
    default_bitmap:Option<NonNull<c_void>>,
    pixels:Option<NonNull<u8>>,
    size:Option<(i32,i32)>,
}
impl Surface {
    fn create()->Result<Self,&'static str>{
        let dc=unsafe{CreateCompatibleDC(ptr::null_mut())};
        Ok(Self{
            dc:NonNull::new(dc).ok_or("Cannot create native bezel memory DC")?,
            bitmap:None,default_bitmap:None,pixels:None,size:None,
        })
    }
    fn resize(&mut self,width:i32,height:i32)->Result<(),&'static str>{
        if self.size==Some((width,height)){return Ok(());}
        if width<=0 || height<=0 || width>MAX_SIDE || height>MAX_SIDE ||
            i64::from(width)*i64::from(height)>MAX_PIXELS {
            return Err("Bezel surface size is invalid");
        }
        let info=BitmapInfo{
            header:BitmapInfoHeader{
                size:std::mem::size_of::<BitmapInfoHeader>() as u32,
                width,height:-height,planes:1,bits_per_pixel:32,compression:0,
                size_image:(i64::from(width)*i64::from(height)*4) as u32,
                ppm_x:0,ppm_y:0,colors_used:0,colors_important:0,
            },
            colors:[0],
        };
        let mut bits=ptr::null_mut();
        let bitmap=unsafe{
            CreateDIBSection(ptr::null_mut(),&info,DIB_RGB_COLORS,
                &mut bits,ptr::null_mut(),0)
        };
        let bitmap=NonNull::new(bitmap).ok_or("Cannot allocate bezel BGRA DIB")?;
        let Some(pixels)=NonNull::new(bits as *mut u8) else {
            unsafe{DeleteObject(bitmap.as_ptr());}
            return Err("Bezel DIB returned no pixel buffer");
        };
        let previous=unsafe{SelectObject(self.dc.as_ptr(),bitmap.as_ptr())};
        let Some(previous)=NonNull::new(previous).filter(|p|p.as_ptr() as isize !=-1) else {
            unsafe{DeleteObject(bitmap.as_ptr());}
            return Err("Cannot select bezel DIB in memory DC");
        };
        if self.bitmap.is_none(){self.default_bitmap=Some(previous);}
        if let Some(old)=self.bitmap.replace(bitmap){
            unsafe{DeleteObject(old.as_ptr());}
        }
        self.pixels=Some(pixels);
        self.size=Some((width,height));
        Ok(())
    }
    fn present(&mut self,hwnd:*mut c_void,layout:CaseLayout,camera_right:bool)
        ->Result<(),&'static str>
    {
        let bgra=render_bezel(layout,camera_right)?;
        self.resize(layout.width,layout.height)?;
        let bits=self.pixels.ok_or("Bezel bitmap buffer is missing")?;
        unsafe{ptr::copy_nonoverlapping(bgra.as_ptr(),bits.as_ptr(),bgra.len());}
        let point=layout.point;
        let size=Size{cx:layout.width,cy:layout.height};
        let zero=Point::default();
        let blend=Blend{
            operation:0,flags:0,constant_alpha:255,alpha_format:AC_SRC_ALPHA
        };
        let screen=unsafe{GetDC(ptr::null_mut())};
        if screen.is_null(){return Err("No Windows desktop DC for bezel");}
        let result=unsafe{
            UpdateLayeredWindow(hwnd,screen,&point,&size,self.dc.as_ptr(),
                &zero,0,&blend,ULW_ALPHA)
        };
        unsafe{ReleaseDC(ptr::null_mut(),screen);}
        if result==0{return Err("Windows refused native layered bezel pixels");}
        Ok(())
    }
}
impl Drop for Surface{
    fn drop(&mut self){
        unsafe{
            if let Some(default)=self.default_bitmap{
                SelectObject(self.dc.as_ptr(),default.as_ptr());
            }
            if let Some(bitmap)=self.bitmap{DeleteObject(bitmap.as_ptr());}
            DeleteDC(self.dc.as_ptr());
        }
    }
}

struct NativeWorker{
    pid:u32,
    stop:Arc<AtomicBool>,
    alive:Arc<AtomicBool>,
    thread:Option<JoinHandle<()>>,
}
static ACTIVE:OnceLock<Mutex<Option<NativeWorker>>>=OnceLock::new();
static USB_ACTIVE:OnceLock<Mutex<Option<NativeWorker>>>=OnceLock::new();
fn active(usb:bool)->&'static Mutex<Option<NativeWorker>>{
    let slot=if usb{&USB_ACTIVE}else{&ACTIVE};
    slot.get_or_init(||Mutex::new(None))
}
pub fn active_for(pid:u32)->bool{
    active(false).lock().ok().and_then(|worker|worker.as_ref()
        .map(|w|w.pid==pid && w.alive.load(Ordering::Acquire)))
        .unwrap_or(false)
}
pub fn close(app:&AppHandle){
    close_frame(app,false);
}
pub(crate) fn close_usb(app:&AppHandle){
    close_frame(app,true);
}
fn close_frame(app:&AppHandle,usb:bool){
    let worker=active(usb).lock().ok().and_then(|mut w|w.take());
    if let Some(mut worker)=worker{
        worker.stop.store(true,Ordering::Release);
        if let Some(thread)=worker.thread.take(){let _=thread.join();}
    }
    if !usb {if let Some(state)=app.try_state::<crate::ios_airplay::IosAirplayState>(){
        state.frame_attached.store(false,Ordering::Release);
        if let Ok(mut error)=state.frame_error.lock(){*error=None;}
    }}
    crate::device_frame::close(app,ios_key(usb));
}
pub fn start(app:&AppHandle,pid:u32)->Result<(),String>{
    start_frame(app,pid,false)
}
pub(crate) fn start_usb(app:&AppHandle,pid:u32)->Result<(),String>{
    start_frame(app,pid,true)
}
fn ios_key(usb:bool)->&'static str{
    if usb {crate::device_frame::IOS_USB_KEY}else{crate::device_frame::IOS_MIRROR_KEY}
}
fn start_frame(app:&AppHandle,pid:u32,usb:bool)->Result<(),String>{
    close_frame(app,usb);
    crate::ios_airplay::load_camera_position(app)?;
    let pref=crate::ios_airplay::read_pref(app,position_pref_file(usb),"on",
        |value|RememberPosition::parse(value).is_some());
    let position=match pref {
        Ok(value)=>RememberPosition::parse(&value).unwrap_or(RememberPosition::Enabled(None)),
        Err(error)=>{
            eprintln!("[iOS window position] {error}");
            RememberPosition::Enabled(None)
        }
    };
    *POSITIONS[usize::from(usb)].lock().map_err(|e|e.to_string())?=position;
    let stop=Arc::new(AtomicBool::new(false));
    let alive=Arc::new(AtomicBool::new(true));
    let stop_worker=stop.clone();
    let alive_worker=alive.clone();
    let app_worker=app.clone();
    let thread=thread::Builder::new().name("ios-native-bezel".into())
        .spawn(move||{
            if let Err(reason)=run_native(app_worker.clone(),pid,&stop_worker,usb){
                if usb {eprintln!("[iOS USB frame] {reason}");}
                else if let Some(state)=app_worker.try_state::<crate::ios_airplay::IosAirplayState>(){
                    state.frame_attached.store(false,Ordering::Release);
                    if let Ok(mut error)=state.frame_error.lock(){
                        *error=Some(reason.to_string());
                    }
                }
            }
            alive_worker.store(false,Ordering::Release);
        }).map_err(|e|format!("Cannot start native iOS bezel worker: {e}"))?;
    *active(usb).lock().map_err(|e|e.to_string())?=Some(NativeWorker{
        pid,stop,alive,thread:Some(thread)
    });
    if let Err(error)=crate::device_frame::start(app,ios_key(usb),false,false){
        eprintln!("[iOS frame] Toolbar unavailable: {error}");
    }
    Ok(())
}
fn pump_messages(){
    let mut msg=WinMsg::default();
    while unsafe{PeekMessageW(&mut msg,ptr::null_mut(),0,0,1)}!=0{
        unsafe{TranslateMessage(&msg);DispatchMessageW(&msg);}
    }
}
fn hide(hwnd:*mut c_void,shown:&mut bool,usb:bool){
    let index=usize::from(usb);
    if let Ok(mut visible)=VISIBLE_BEZELS[index].lock(){
        *visible=None;
    }
    HOOK_MOVE_ENDED[index].store(false,Ordering::Release);
    if *shown{unsafe{ShowWindow(hwnd,SW_HIDE);}}
    *shown=false;
}
fn show(hwnd:*mut c_void,shown:&mut bool){
    if !*shown{unsafe{ShowWindow(hwnd,SW_SHOWNOACTIVATE);}}
    *shown=true;
}
fn set_error(app:&AppHandle,reason:Option<&str>){
    if let Some(state)=app.try_state::<crate::ios_airplay::IosAirplayState>(){
        if let Ok(mut error)=state.frame_error.lock(){
            if error.as_deref()!=reason{*error=reason.map(str::to_owned);}
        }
    }
}
fn moved_video_target(screen:VideoClient,inset:i32,expected_w:i32,expected_h:i32)
    ->Option<Point>
{
    if screen.width!=expected_w || screen.height!=expected_h{return None;}
    Some(Point{
        x:screen.point.x.checked_sub(inset)?,
        y:screen.point.y.checked_sub(inset)?,
    })
}
fn move_existing_bezel(bezel:*mut c_void,target:Point)->bool{
    (unsafe{SetWindowPos(bezel,ptr::null_mut(),target.x,target.y,0,0,
        SWP_NOSIZE|SWP_NOZORDER|SWP_NOACTIVATE)})!=0
}
/// Out-of-context accessibility location event. It executes on the native
/// bezel worker's message pump (no Windows cross-process DLL injection).
/// The only affected HWND is our own click-through native layer.
unsafe extern "system" fn on_video_location_change(
    _hook:*mut c_void,event:u32,hwnd:*mut c_void,
    object_id:i32,child_id:i32,_thread:u32,_time:u32,
){
    if object_id!=OBJID_WINDOW || child_id!=CHILDID_SELF{return;}
    let Some((index,target))=VISIBLE_BEZELS.iter().enumerate().find_map(|(index,slot)|
        slot.lock().ok().and_then(|target|*target)
            .filter(|target|target.video==hwnd as usize).map(|target|(index,target))
    )else{return;};
    if event==EVENT_SYSTEM_MOVESIZEEND{
        HOOK_MOVE_ENDED[index].store(true,Ordering::Release);
    }
    if crate::device_frame::is_dragging(ios_key(index==1)){return;}
    let Some(screen)=video_client(HWND(hwnd)) else{return;};
    let Some(point)=moved_video_target(
        screen,target.inset,target.video_width,target.video_height,
    )else{return;};
    move_existing_bezel(target.window as *mut c_void,point);
}
struct NativeLocationHook([*mut c_void;2]);
impl NativeLocationHook{
    fn install(pid:u32)->Self{
        let hooks=[EVENT_OBJECT_LOCATIONCHANGE,EVENT_SYSTEM_MOVESIZEEND].map(|event|unsafe{SetWinEventHook(
            event,event,
            ptr::null_mut(),Some(on_video_location_change),
            pid,0,WINEVENT_OUTOFCONTEXT
        )});
        Self(hooks)
    }
}
impl Drop for NativeLocationHook{
    fn drop(&mut self){
        for hook in self.0{
            if !hook.is_null(){unsafe{UnhookWinEvent(hook);}}
        }
    }
}
// Window creation, updates and destruction all run on the owning thread.
struct WindowOwner(NonNull<c_void>);
impl Drop for WindowOwner{
    fn drop(&mut self){unsafe{DestroyWindow(self.0.as_ptr());}}
}
fn run_native(app:AppHandle,pid:u32,stop:&AtomicBool,usb:bool)
    ->Result<(),&'static str>
{
    let _physical_dpi=ThreadDpi::enter(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2)
        .ok_or("Cannot set physical DPI context for the AirPlay frame worker")?;
    // Static Win32 window class is already registered by user32. This HWND
    // belongs entirely to the native worker, never to WebView2 or another PID.
    let class:Vec<u16>="STATIC".encode_utf16().chain(Some(0)).collect();
    let title:Vec<u16>="MirroLeap iOS native device bezel".encode_utf16()
        .chain(Some(0)).collect();
    let hwnd=unsafe{
        CreateWindowExW(
            EX_LAYERED|EX_TRANSPARENT|EX_TOOLWINDOW|EX_NOACTIVATE,
            class.as_ptr(),title.as_ptr(),WS_POPUP,
            0,0,1,1,ptr::null_mut(),ptr::null_mut(),
            ptr::null_mut(),ptr::null_mut())
    };
    let Some(hwnd)=NonNull::new(hwnd) else{
        return Err("无法创建 Windows 原生透明边框窗口");
    };
    let _window=WindowOwner(hwnd);
    let _location_hook=NativeLocationHook::install(pid);
    let index=usize::from(usb);
    let mut surface=Surface::create()?;
    let mut controls=(!usb).then(||crate::ios_window_controls::NativeControls::create().ok()).flatten();
    let mut previous_skin=None;
    let mut applied_video_clip:Option<NativeVideoClip>=None;
    let mut receiver_dwm:Option<ReceiverDwmStyle>=None;
    let mut checked_dwm_at=Instant::now()-Duration::from_secs(3);
    let mut clip_checked_at=Instant::now()-Duration::from_secs(1);
    let mut clip_recheck_until=None;
    let mut last_stacked=Instant::now()-Duration::from_secs(1);
    let mut last_anchor:Option<usize>=None;
    let mut shown=false;
    let mut applied=false;
    let mut last_size_request=None;
    let mut resize_after=None;
    let mut first_frame_at=None;
    let mut position_window=None;
    let mut pending_position:Option<(Point,Instant)>=None;
    let mut was_dragging=false;
    while !stop.load(Ordering::Acquire){
        pump_messages();
        match (!usb).then(crate::ios_window_controls::NativeControls::action).flatten(){
            Some(crate::ios_window_controls::ControlAction::Minimize)=>{
                if let Some(video)=crate::commands::find_window_by_pid(pid){
                    crate::ios_window_controls::NativeControls::minimize_receiver(video);
                }
            }
            Some(crate::ios_window_controls::ControlAction::Close)=>{
                // Receiver shutdown joins this bezel worker. Run it on a
                // different thread to avoid self-join deadlocks.
                let owner=app.clone();
                let _=thread::Builder::new().name("ios-receiver-close".into())
                    .spawn(move||crate::ios_airplay::shutdown(&owner));
                break;
            }
            None=>{},
        }
        let Some(state)=app.try_state::<crate::ios_airplay::IosAirplayState>()
            else{break};
        let usb_video=crate::ios_usb::video().filter(|(active,_)|*active==pid);
        let running=if usb{usb_video.is_some()}else{crate::ios_airplay::active_receiver_pid(&state)==Some(pid)};
        if !running{break;}
        let style=if usb{"device-auto".into()}else{
            state.window_style.lock().ok().map(|s|s.clone()).unwrap_or_default()
        };
        let set_attached=|attached|{if !usb{state.frame_attached.store(attached,Ordering::Release);}};
        let set_frame_error=|reason|{if !usb{set_error(&app,reason);}};
        let managed=(!usb).then(||managed_receiver_window(&state,pid)).flatten();
        let video=managed.or_else(||crate::commands::find_window_by_pid(pid));
        let Some(video)=video else{
            if let Some((point,_))=pending_position.take(){persist_window_position(&app,usb,point);}
            position_window=None;
            receiver_dwm=None; // The former HWND vanished; never touch a recycled handle.
            if let Some(control)=controls.as_mut(){control.hide();}
            hide(hwnd.as_ptr(),&mut shown,usb);
            set_attached(false);
            thread::sleep(Duration::from_millis(50));
            continue;
        };
        let source=if usb{usb_video.map(|(_,pixels)|pixels)}else{state.source_pixels.lock().ok().and_then(|pixels|*pixels)};
        if managed.is_some() && source.is_none(){
            if let Some((point,_))=pending_position.take(){persist_window_position(&app,usb,point);}
            position_window=None;
            hide(hwnd.as_ptr(),&mut shown,usb);
            if let Some(control)=controls.as_mut(){control.hide();}
            if unsafe{IsWindowVisible(video)}.as_bool(){
                clear_clip_if_still_owned(pid,&mut applied_video_clip);
                unsafe{ShowWindow(video.0,SW_HIDE);}
            }
            set_attached(false);
            last_size_request=None;
            resize_after=None;
            first_frame_at=None;
            thread::sleep(Duration::from_millis(32));
            continue;
        }
        let hidden_live=managed.is_some() && !unsafe{IsWindowVisible(video)}.as_bool();
        if hidden_live {
            let first=*first_frame_at.get_or_insert_with(Instant::now);
            // Allow the receiver's first texture upload and aspect change
            // to finish while Windows still keeps its HWND invisible.
            if first.elapsed()<Duration::from_millis(120){
                thread::sleep(Duration::from_millis(16));
                continue;
            }
            if first.elapsed()>=Duration::from_millis(1200){
                // A failed optional bezel must not swallow a live stream.
                // The display guard still requires a frame for this PID.
                show_live_receiver(&state,pid,video);
            }
        }else{
            first_frame_at=None;
        }
        let screen=if hidden_live {client_rect(video)}else{video_client(video)};
        let Some(screen)=screen else{
            if let Some(control)=controls.as_mut(){control.hide();}
            hide(hwnd.as_ptr(),&mut shown,usb);
            set_attached(false);
            thread::sleep(Duration::from_millis(40));
            continue;
        };
        let dragging=crate::device_frame::is_dragging(ios_key(usb));
        if dragging && shown{
            was_dragging=true;
            // The toolbar drag moves video, bezel and tools in one batch.
            // Keep pumping messages so that DeferWindowPos can complete.
            unsafe{MsgWaitForMultipleObjectsEx(None,FOLLOW_MS,QS_ALLINPUT,MWMO_INPUTAVAILABLE);}
            continue;
        }
        let display=state.display_size.lock().ok().map(|size|*size).unwrap_or_default();
        let owned=!usb && state.child.lock().ok().is_some_and(|child|child.as_ref().is_some_and(|child|child.id()==pid));
        let fullscreen=is_fullscreen(video);
        if usb || (owned && style!="classic") {
            let request=source.map(|dimensions|(dimensions,display));
            if last_size_request!=Some(request){
                last_size_request=Some(request);
                // Let the receiver finish its own aspect change before fitting it to the desktop.
                resize_after=Some(if usb || hidden_live{Instant::now()}else{Instant::now()+Duration::from_millis(160)});
            }
            if !fullscreen && resize_after.is_some_and(|at|Instant::now()>=at){
                resize_after=None;
                if resize_for_source(video,screen,source,display){continue;}
            }
        }
        if source.is_some() && resize_after.is_none() && !fullscreen {
            if let Some(bounds)=position_bounds(video){
                if position_window!=Some(video.0 as usize) {
                    position_window=Some(video.0 as usize);
                    let saved=POSITIONS[usize::from(usb)].lock().ok().map(|p|*p);
                    if let Some(RememberPosition::Enabled(Some(point)))=saved {
                        if restore_window_position(video,bounds,point){continue;}
                    }
                }
                if !hidden_live {
                    let point=Point{x:bounds.left,y:bounds.top};
                    match pending_position {
                        Some((last,at)) if last==point=>{
                            if at.elapsed()>=Duration::from_millis(250){
                                persist_window_position(&app,usb,point);
                                pending_position=None;
                            }
                        }
                        _=>pending_position=Some((point,Instant::now())),
                    }
                }
            }
        }
        // Two native buttons are available even in "clean" mode; control
        // only this AirPlay receiver, never the Android mirror/main GUI.
        if let Some(control)=controls.as_mut(){
            if hidden_live || (style.starts_with("device-") && !fullscreen && crate::device_frame::ios_toolbar_created(&app)){
                control.hide();
            }else{
                control.update_hover(video,screen.point.x,screen.point.y,screen.width);
            }
        }
        if !style.starts_with("device-") || fullscreen{
            clear_clip_if_still_owned(pid,&mut applied_video_clip);
            if let Some(override_style)=receiver_dwm.take()
                .filter(|override_style|override_style.hwnd==video.0 as usize){
                override_style.restore();
            }
            hide(hwnd.as_ptr(),&mut shown,usb);
            set_attached(false);
            if hidden_live{show_live_receiver(&state,pid,video);}
            thread::sleep(Duration::from_millis(40));
            continue;
        }
        if let Some(override_style)=receiver_dwm.as_ref()
            .filter(|override_style|override_style.hwnd==video.0 as usize){
            if checked_dwm_at.elapsed()>Duration::from_secs(2){
                override_style.refresh(video);
                checked_dwm_at=Instant::now();
            }
        }else{
            receiver_dwm=Some(ReceiverDwmStyle::attach(video));
            checked_dwm_at=Instant::now();
        }
        let dpi=unsafe{GetDpiForWindow(video)}.clamp(96,384);
        let case_style=if style=="device-auto" && (owned || usb){
            if source.is_some_and(|(w,h)|choose_ipad(&style,w as i32,h as i32)) {"device-ipad"}
            else {"device-iphone"}
        }else{&style};
        let Some(layout)=target_layout(screen,case_style,dpi) else{
            hide(hwnd.as_ptr(),&mut shown,usb);
            set_attached(false);
            set_frame_error(Some("视频窗口过大，已安全隐藏真机边框"));
            thread::sleep(Duration::from_millis(80));
            continue;
        };
        let now=Instant::now();
        let move_ended=HOOK_MOVE_ENDED[index].swap(false,Ordering::AcqRel) ||
            std::mem::take(&mut was_dragging);
        if move_ended{
            // SDL may reset its region just after Windows exits the move loop.
            clip_recheck_until=Some(now+Duration::from_millis(MOVE_END_CHECK_MS));
        }
        let check_clip=clip_recheck_until.is_some_and(|until|now<=until) ||
            now.duration_since(clip_checked_at)>Duration::from_millis(650);
        let mut clip_reapplied=false;
        if let Some(clip)=clip_signature(video,screen,layout.ipad,dpi).filter(|clip|
            applied_video_clip!=Some(*clip) ||
                (check_clip && !native_clip_intact(video,screen,*clip))) {
            if let Err(error)=apply_video_corner_clip(video,screen,clip){
                clear_clip_if_still_owned(pid,&mut applied_video_clip);
                hide(hwnd.as_ptr(),&mut shown,usb);
                set_attached(false);
                set_frame_error(Some(error));
                thread::sleep(Duration::from_millis(200));
                continue;
            }
            applied_video_clip=Some(clip);
            clip_reapplied=true;
        }
        if check_clip{
            clip_checked_at=now;
        }
        let camera_right=layout.width>layout.height && state.landscape_camera_right
            .lock().is_ok_and(|right|*right);
        let skin=(layout.width,layout.height,layout.ipad,layout.dpi,camera_right);
        let skin_changed=previous_skin!=Some(skin);
        if skin_changed{
            if let Err(reason)=surface.present(hwnd.as_ptr(),layout,camera_right){
                hide(hwnd.as_ptr(),&mut shown,usb);
                set_attached(false);
                set_frame_error(Some(reason));
                thread::sleep(Duration::from_millis(250));
                continue;
            }
            previous_skin=Some(skin);
        }
        // Previously we forcibly re-stacked this top-level layer EVERY 16ms,
        // competing with the immediate EVENT_OBJECT_LOCATIONCHANGE handler.
        // Let that callback do movement in the same Win32 event turn, and
        // perform only light no-z-order repairs during the health loop.
        // Re-stack at 350ms intervals / geometry or anchor changes.
        let restack=move_ended || clip_reapplied || skin_changed || !shown ||
            last_anchor!=Some(video.0 as usize) ||
            last_stacked.elapsed()>Duration::from_millis(350);
        let location=if restack{
            last_stacked=Instant::now();
            last_anchor=Some(video.0 as usize);
            crate::device_frame::place_ios_companion(
                HWND(hwnd.as_ptr()),video,layout.point.x,layout.point.y,
                layout.width,layout.height)
        }else{
            let mut old=RECT::default();
            if unsafe{GetWindowRect(HWND(hwnd.as_ptr()),&mut old)}.is_err(){
                Err("Could not inspect native bezel position".into())
            }else if (old.left-layout.point.x).abs()>1 ||
                (old.top-layout.point.y).abs()>1 {
                if !move_existing_bezel(hwnd.as_ptr(),layout.point){
                    Err("Could not repair native bezel position".into())
                }else{Ok(())}
            }else{Ok(())}
        };
        if location.is_err(){
            hide(hwnd.as_ptr(),&mut shown,usb);
            set_attached(false);
            set_frame_error(Some("Windows 拒绝将原生边框放在投屏窗口上方"));
            thread::sleep(Duration::from_millis(100));
            continue;
        }
        let mut actual=RECT::default();
        let hwnd_readable=unsafe{
            GetWindowRect(HWND(hwnd.as_ptr()),&mut actual)
        }.is_ok();
        if !hwnd_readable ||
            (actual.left-layout.point.x).abs()>1 ||
            (actual.top-layout.point.y).abs()>1 ||
            (actual.right-actual.left-layout.width).abs()>1 ||
            (actual.bottom-actual.top-layout.height).abs()>1 {
            hide(hwnd.as_ptr(),&mut shown,usb);
            set_attached(false);
            set_frame_error(Some("原生外壳位置验证失败，已隐藏以防止边框分离"));
            thread::sleep(Duration::from_millis(45));
            continue;
        }
        // Transparent centre is guaranteed by BGRA alpha=0, not by WebView
        // CSS, SetParent or DPI-virtualized browser client coordinates.
        if hidden_live && !show_live_receiver(&state,pid,video){continue;}
        show(hwnd.as_ptr(),&mut shown);
        if let Ok(mut visible)=VISIBLE_BEZELS[index].lock(){
            *visible=Some(VisibleBezel{video:video.0 as usize,window:hwnd.as_ptr() as usize,
                inset:bezel_inset(layout.ipad,layout.dpi),video_width:screen.width,video_height:screen.height});
        }
        if !usb{
            set_attached(true);
            if !applied{set_frame_error(None);applied=true;}
        }
        // Wake for out-of-context WinEvents as soon as Windows delivers them.
        // When nothing moves, the timeout still supports the health checks.
        unsafe{MsgWaitForMultipleObjectsEx(None,FOLLOW_MS,QS_ALLINPUT,MWMO_INPUTAVAILABLE);}
    }
    hide(hwnd.as_ptr(),&mut shown,usb);
    if let Some((point,_))=pending_position{persist_window_position(&app,usb,point);}
    clear_clip_if_still_owned(pid,&mut applied_video_clip);
    // Restore the receiver's original Windows frame preferences only if
    // its original HWND is still owned by this AirPlay process.
    if let Some(style)=receiver_dwm{
        if crate::commands::find_window_by_pid(pid)
            .is_some_and(|hwnd|hwnd.0 as usize==style.hwnd){
            style.restore();
        }
    }
    if !usb {if let Some(state)=app.try_state::<crate::ios_airplay::IosAirplayState>(){
        state.frame_attached.store(false,Ordering::Release);
    }}
    Ok(())
}

#[cfg(test)]
mod tests{
    use super::*;

    #[test]
    fn remembered_position_is_signed_and_clamped_to_the_current_monitor(){
        assert_eq!(RememberPosition::parse("off"),Some(RememberPosition::Disabled));
        assert_eq!(RememberPosition::parse("on"),Some(RememberPosition::Enabled(None)));
        let point=Point{x:-1700,y:-45};
        assert_eq!(RememberPosition::parse("-1700,-45"),Some(RememberPosition::Enabled(Some(point))));
        assert_eq!(RememberPosition::parse("1000001,0"),None);
        let window=RECT{left:0,top:0,right:400,bottom:800};
        let side_monitor=RECT{left:-1920,top:-150,right:0,bottom:930};
        assert_eq!(constrain_position(point,window,side_monitor),point);
        let primary=RECT{left:0,top:0,right:1920,bottom:1040};
        assert_eq!(constrain_position(point,window,primary),Point{x:0,y:0});
    }
    fn layout(w:i32,h:i32,ipad:bool,dpi:u32)->CaseLayout{
        CaseLayout{point:Point{x:100,y:150},width:w,height:h,ipad,dpi}
    }
    #[test]
    fn waiting_phone_and_connected_iphone_ipad_fit_the_actual_aspect(){
        assert_eq!(source_size(None,DisplaySize::Large,1.0,(1920,1080)),(360,780));
        let (phone_w,phone_h)=source_size(Some((1170,2535)),DisplaySize::Adaptive,1.5,(3840,2160));
        assert!(phone_h>1170);
        assert!((phone_w as f64/phone_h as f64-1170.0/2535.0).abs()<0.001);
        let (w,h)=source_size(Some((1640,2360)),DisplaySize::Large,1.5,(1920,1080));
        assert_eq!(h,951);
        assert!((w as f64/h as f64-1640.0/2360.0).abs()<0.001);
        let rotated=source_size(Some((2360,1640)),DisplaySize::Large,1.5,(1920,1080));
        assert_eq!(rotated,(h,w));
    }

    #[test]
    fn quality_window_sizes_remain_distinct_on_small_displays(){
        let source=Some((1640,2360));
        let narrow=(1366,768);
        let sizes=[DisplaySize::Small,DisplaySize::Adaptive,DisplaySize::Medium,DisplaySize::Large]
            .map(|display|source_size(source,display,1.5,narrow));
        assert!(sizes.windows(2).all(|pair|pair[0].0<pair[1].0 && pair[0].1<pair[1].1));
        assert_eq!(source_size(Some((1080,1920)),DisplaySize::Source,1.5,(3840,2160)),(1080,1920));
        for display in [DisplaySize::Small,DisplaySize::Adaptive,DisplaySize::Medium,
            DisplaySize::Large,DisplaySize::Source]{
            for work in [(1920,1080),(1080,1920),(1366,768)]{
                for (w,h) in [(1170,2535),(1640,2360)]{
                    let (short,long)=source_size(Some((w,h)),display,1.5,work);
                    assert_eq!(source_size(Some((h,w)),display,1.5,work),(long,short));
                }
            }
        }
    }

    #[test]
    fn adaptive_window_matches_usb_baseline_across_decoded_resolutions(){
        let work=(3440,1440);
        let expected=(791,1139);
        for source in [(1640,2360),(820,1180),(2460,3540)] {
            assert_eq!(source_size(Some(source),DisplaySize::Adaptive,1.5,work),expected);
            assert_eq!(source_size(Some((source.1,source.0)),DisplaySize::Adaptive,1.5,work),
                (expected.1,expected.0));
        }
    }

    #[test]
    fn compact_start_fits_a_hidden_landscape_sdl_window_before_reveal(){
        use windows::core::PCWSTR;
        use windows::Win32::Foundation::{HINSTANCE,LPARAM,LRESULT,WPARAM};
        use windows::Win32::System::LibraryLoader::GetModuleHandleW;
        use windows::Win32::UI::WindowsAndMessaging::{DefWindowProcW,RegisterClassW,UnregisterClassW,WNDCLASSW};
        unsafe extern "system" fn window_proc(hwnd:HWND,message:u32,wparam:WPARAM,lparam:LPARAM)->LRESULT{
            unsafe{DefWindowProcW(hwnd,message,wparam,lparam)}
        }
        let _dpi=ThreadDpi::enter(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2).unwrap();
        let name:Vec<u16>="SDL_app".encode_utf16().chain(Some(0)).collect();
        let name_ptr=PCWSTR(name.as_ptr());
        let instance=unsafe{GetModuleHandleW(None)}.unwrap();
        let instance=HINSTANCE(instance.0);
        let class=WNDCLASSW{lpfnWndProc:Some(window_proc),hInstance:instance,lpszClassName:name_ptr,
            ..Default::default()};
        assert_ne!(unsafe{RegisterClassW(&class)},0);
        let raw=unsafe{CreateWindowExW(0,name.as_ptr(),name.as_ptr(),WS_POPUP,
            100,100,1240,800,ptr::null_mut(),ptr::null_mut(),instance.0,ptr::null_mut())};
        let window=WindowOwner(NonNull::new(raw).unwrap());
        let hwnd=HWND(raw);
        let pid=std::process::id();
        let fitted=prepare_background_window(pid,true).unwrap();
        let visible=unsafe{IsWindowVisible(hwnd)}.as_bool();
        let size=client_rect(hwnd).unwrap();
        let state=crate::ios_airplay::IosAirplayState::default();
        *state.managed_window.lock().unwrap()=Some((pid,fitted));
        assert!(!show_live_receiver(&state,pid,hwnd));
        assert!(!unsafe{IsWindowVisible(hwnd)}.as_bool());
        // A real source may open the window. Keep this synthetic one off
        // screen, and close it before releasing the test class.
        unsafe{SetWindowPos(raw,ptr::null_mut(),-40000,-40000,0,0,SWP_NOSIZE|SWP_NOZORDER|SWP_NOACTIVATE);}
        *state.source_pixels.lock().unwrap()=Some((1640,2360));
        assert!(show_live_receiver(&state,pid,hwnd));
        drop(window);
        unsafe{UnregisterClassW(name_ptr,Some(instance))}.unwrap();
        assert_eq!(fitted,hwnd.0 as usize);
        assert!(!visible);
        assert!(size.width<size.height && size.width<1240);
        assert!((size.width as f64/size.height as f64-360.0/780.0).abs()<0.01);
    }

    #[test]
    fn metal_finish_has_intermediate_pixels_instead_of_a_two_color_step(){
        let silver=metal_band(1.6,1.0,0.0).0;
        let middle=metal_band(2.1,1.0,0.0).0;
        let graphite=metal_band(2.65,1.0,0.0).0;
        assert!(silver>middle && middle>graphite);
    }
    #[test]
    fn native_layer_does_not_reparent_tauri_or_modify_any_webview(){
        assert!(!active_for(0));
        assert_eq!(bezel_inset(true,96),12);
        assert_eq!(bezel_inset(true,144),18);
        assert_eq!(bezel_inset(false,192),36);
        assert!(choose_ipad("device-auto",1024,1366));
        assert!(!choose_ipad("device-auto",500,1120));
    }
    #[test]
    fn hollow_rim_preserves_entire_unscaled_video_centre(){
        let view=layout(760,1080,true,96);
        let bgra=render_bezel(view,false).unwrap();
        assert_eq!(bgra.len(),760*1080*4);
        let centre=((540*760+380)*4) as usize;
        assert_eq!(&bgra[centre..centre+4],&[0,0,0,0]);
        let top=((1*760+380)*4) as usize;
        assert!(bgra[top+3]>0);
        // Camera is part of the bezel, never a rendered video pixel.
        let inside=((30*760+380)*4) as usize;
        assert_eq!(bgra[inside+3],0);
    }
    #[test]
    fn android_style_native_overscan_seals_all_four_wallpaper_seams(){
        let skin=layout(760,1080,true,96);
        let bgra=render_bezel(skin,false).unwrap();
        let inset=bezel_inset(true,96);
        let overlap=seam_cover_px(96);
        assert_eq!(overlap,crate::frame_layout::BEZEL_CLIP_OVERSCAN_PX+3);
        let alpha=|x:i32,y:i32|bgra[((y*skin.width+x)*4+3) as usize];
        // Original SDL pixels begin exactly at inset. Opaque graphite now
        // overlaps the first FOUR video pixels instead of exposing desktop.
        for step in 0..overlap {
            assert_eq!(alpha(inset+step,skin.height/2),255);
            assert_eq!(alpha(skin.width-inset-1-step,skin.height/2),255);
            assert_eq!(alpha(skin.width/2,inset+step),255);
            assert_eq!(alpha(skin.width/2,skin.height-inset-1-step),255);
        }
        assert_eq!(alpha(skin.width/2,skin.height/2),0);
    }
    #[test]
    fn ipad_air_legacy_graphite_silver_hairline_not_cyan_glow(){
        let skin=layout(420,820,true,96);
        let bgra=render_bezel(skin,false).unwrap();
        let p=(skin.height/2*skin.width*4) as usize;
        let (blue,green,red,alpha)=(bgra[p],bgra[p+1],bgra[p+2],bgra[p+3]);
        assert_eq!(alpha,255);
        assert!(red>=200 && green>=200 && blue>=200);
        assert!(bgra[p+4+2]<180,"the inner pixel should fall into the darker bevel");
        assert!(blue.saturating_sub(red)<32);
    }
    #[test]
    fn silver_metal_contour_is_continuous_across_all_four_curved_corners(){
        for ipad in [true,false]{
            for dpi in [96,144,192]{
                let skin=layout(620,940,ipad,dpi);
                let pixels=render_bezel(skin,false).unwrap();
                let radius=BezelGeometry::new(ipad,dpi).outer_radius as f32;
                let inset=1.2*dpi as f32/96.0;
                for degrees in [20.0f32,35.0,50.0,65.0,80.0]{
                    let rad=degrees.to_radians();
                    let x=(radius-(radius-inset)*rad.cos()).round() as i32;
                    let y=(radius-(radius-inset)*rad.sin()).round() as i32;
                    for (cx,cy) in [
                        (x,y),
                        (skin.width-1-x,y),
                        (x,skin.height-1-y),
                        (skin.width-1-x,skin.height-1-y),
                    ]{
                        // The contour is subpixel-antialiased. Its nearest
                        // silver sample may be +/- 1 or 2 integer pixels
                        // from the analytic arc (especially at 125/150% DPI).
                        // Verify local *continuous metal*, not that a single
                        // pixel rounded toward the graphite interior.
                        let silver=(-2..=2).any(|dy|(-2..=2).any(|dx|{
                            let xx=(cx+dx).clamp(0,skin.width-1);
                            let yy=(cy+dy).clamp(0,skin.height-1);
                            let idx=((yy*skin.width+xx)*4) as usize;
                            let (b,g,r,a)=(
                                pixels[idx],pixels[idx+1],
                                pixels[idx+2],pixels[idx+3]
                            );
                            a>=190 && r>=120 && g>=140 && b>=145 && b>r
                        }));
                        assert!(silver,
                            "missing curved silver metal near {cx},{cy} {dpi}dpi ipad={ipad}");
                    }
                }
            }
        }
    }
    #[test]
    fn metal_round_caps_leave_only_the_correct_transparent_outside(){
        let skin=layout(620,940,true,96);
        let pixels=render_bezel(skin,false).unwrap();
        let alpha=|x:i32,y:i32|pixels[((y*skin.width+x)*4+3) as usize];
        // True iPad rounding must not fill the square corners.
        for (x,y) in [(0,0),(skin.width-1,0),
            (0,skin.height-1),(skin.width-1,skin.height-1)] {
            assert_eq!(alpha(x,y),0);
        }
        // Yet every quadrant must contain an uninterrupted metal cap.
        let cap=BezelGeometry::new(true,96).outer_radius;
        for (x,y) in [(cap/2,cap/2),(skin.width-1-cap/2,cap/2),
            (cap/2,skin.height-1-cap/2),
            (skin.width-1-cap/2,skin.height-1-cap/2)] {
            assert!(alpha(x,y)>0);
        }
    }

    #[test]
    fn metal_arc_joins_each_straight_edge_without_an_alpha_step(){
        for dpi in [96,120,144,192]{
            let skin=layout(620,940,true,dpi);
            let pixels=render_bezel(skin,false).unwrap();
            let radius=BezelGeometry::new(true,dpi).outer_radius;
            let alpha=|x,y|pixels[((y*skin.width+x)*4+3) as usize];
            for track in [
                [alpha(radius-4,0),alpha(radius-2,0),alpha(radius,0)],
                [alpha(skin.width-radius+3,0),alpha(skin.width-radius+1,0),alpha(skin.width-radius-1,0)],
                [alpha(radius-4,skin.height-1),alpha(radius-2,skin.height-1),alpha(radius,skin.height-1)],
                [alpha(skin.width-radius+3,skin.height-1),alpha(skin.width-radius+1,skin.height-1),alpha(skin.width-radius-1,skin.height-1)],
                [alpha(0,radius-4),alpha(0,radius-2),alpha(0,radius)],
                [alpha(0,skin.height-radius+3),alpha(0,skin.height-radius+1),alpha(0,skin.height-radius-1)],
                [alpha(skin.width-1,radius-4),alpha(skin.width-1,radius-2),alpha(skin.width-1,radius)],
                [alpha(skin.width-1,skin.height-radius+3),alpha(skin.width-1,skin.height-radius+1),alpha(skin.width-1,skin.height-radius-1)],
            ]{
                assert!(track[0]>=180 && track[0]<=track[1] && track[1]<=track[2] && track[2]>=250);
            }
        }
    }

    #[test]
    fn landscape_camera_can_move_from_left_to_right_without_erasing_the_rim(){
        let landscape=layout(940,620,true,96);
        let left=render_bezel(landscape,false).unwrap();
        let right=render_bezel(landscape,true).unwrap();
        let center=camera_center(landscape,false);
        let pos=((center.1 as i32*landscape.width+center.0 as i32)*4) as usize;
        assert_eq!(&left[pos..pos+4],&[55,42,24,255]);
        assert_ne!(&left[pos..pos+3],&right[pos..pos+3]);
        assert_eq!(left[pos-4*3+3],255);
        let portrait=layout(620,940,true,96);
        assert_eq!(camera_center(portrait,false),camera_center(portrait,true));
    }
    #[test]
    fn location_hook_keeps_bezel_visible_through_rapid_position_changes(){
        let _physical_dpi=ThreadDpi::enter(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2).unwrap();
        let class:Vec<u16>="STATIC".encode_utf16().chain(Some(0)).collect();
        let create_window=|width,height|{
            let raw=unsafe{CreateWindowExW(EX_NOACTIVATE,class.as_ptr(),ptr::null(),
                WS_POPUP,-32000,-32000,width,height,ptr::null_mut(),ptr::null_mut(),
                ptr::null_mut(),ptr::null_mut())};
            let window=WindowOwner(NonNull::new(raw).expect("native drag test HWND"));
            unsafe{ShowWindow(raw,SW_SHOWNOACTIVATE);}
            window
        };
        let video=create_window(320,480);
        let bezel=create_window(348,508);
        let video=video.0.as_ptr();
        let bezel=bezel.0.as_ptr();
        for (index,slot) in VISIBLE_BEZELS.iter().enumerate(){
            *slot.lock().unwrap()=Some(VisibleBezel{
                video:video as usize,window:bezel as usize,inset:14,video_width:320,video_height:480,
            });
            for (x,y) in [(-5000,-5800),(-4800,-5780),(-4875,-5600)]{
                assert!(move_existing_bezel(video,Point{x,y}));
                let client=video_client(HWND(video)).unwrap();
                unsafe{on_video_location_change(ptr::null_mut(),EVENT_OBJECT_LOCATIONCHANGE,
                    video,OBJID_WINDOW,CHILDID_SELF,0,0);}
                assert!(unsafe{IsWindowVisible(HWND(bezel))}.as_bool());
                let mut rect=RECT::default();
                unsafe{GetWindowRect(HWND(bezel),&mut rect)}.unwrap();
                assert_eq!((rect.left,rect.top),(client.point.x-14,client.point.y-14));
                assert_eq!((rect.right-rect.left,rect.bottom-rect.top),(348,508));
            }
            assert!(!HOOK_MOVE_ENDED[index].load(Ordering::Acquire));
            unsafe{on_video_location_change(ptr::null_mut(),EVENT_OBJECT_LOCATIONCHANGE,
                video,OBJID_WINDOW,CHILDID_SELF,0,0);
                on_video_location_change(ptr::null_mut(),EVENT_SYSTEM_MOVESIZEEND,
                    video,OBJID_WINDOW,CHILDID_SELF,0,0);}
            assert!(HOOK_MOVE_ENDED[index].swap(false,Ordering::AcqRel));
            let companion=drag_bezel(index==1,HWND(video)).unwrap();
            let client=video_client(HWND(video)).unwrap();
            let start=(client.point.x,client.point.y);
            for dest in [(-4600,-4700),(-4700,-4900)]{
                crate::device_frame::move_drag_batch(HWND(video),start,dest,&[companion]).unwrap();
                let mut rect=RECT::default();
                unsafe{GetWindowRect(HWND(bezel),&mut rect)}.unwrap();
                assert_eq!((rect.left,rect.top),(dest.0-14,dest.1-14));
                assert!(unsafe{IsWindowVisible(HWND(bezel))}.as_bool());
            }
            *slot.lock().unwrap()=None;
        }
    }
    #[test]
    fn rounded_outer_corner_is_antialiased_and_zero_outside(){
        let bgra=render_bezel(layout(420,820,false,96),false).unwrap();
        assert_eq!(&bgra[..4],&[0,0,0,0]);
        assert_eq!(bgra[((2*420+210)*4+3) as usize]>0,true);
        assert!(render_bezel(layout(6000,6000,true,96),false).is_err());
    }
    #[test]
    fn each_inner_round_corner_is_covered_not_skipped_as_rectangle(){
        for ipad in [true,false] {
            for dpi in [96,144] {
                let skin=layout(520,880,ipad,dpi);
                let pixels=render_bezel(skin,false).unwrap();
                let inset=bezel_inset(ipad,dpi)+seam_cover_px(dpi);
                let alpha=|x:i32,y:i32|pixels[((y*skin.width+x)*4+3) as usize];
                // The old rectangular shortcut made all four inner
                // corners transparent, creating visible square notches.
                for (x,y) in [
                    (inset+2,inset+2),
                    (skin.width-inset-3,inset+2),
                    (inset+2,skin.height-inset-3),
                    (skin.width-inset-3,skin.height-inset-3),
                ] {
                    assert_eq!(alpha(x,y),255,"corner ({x},{y}) at {dpi} DPI");
                }
                assert_eq!(alpha(skin.width/2,skin.height/2),0);
            }
        }
    }
    #[test]
    fn native_video_clip_geometry_has_larger_real_round_corners(){
        let video=VideoClient{
            point:Point{x:100,y:80},width:700,height:980
        };
        let outer=RECT{left:92,top:72,right:808,bottom:1068};
        let (left,top,right,bottom,radius)=clip_shape(video,outer,true,96).unwrap();
        assert_eq!((left,top,right,bottom),(8,8,708,988));
        assert_eq!(radius,BezelGeometry::new(true,96).video_clip_radius());
        let (_,_,_,_,hi_dpi)=clip_shape(video,outer,true,144).unwrap();
        assert!(hi_dpi>radius);
    }
    #[test]
    fn inner_aperture_and_native_video_arcs_are_concentric_at_all_dpis(){
        for ipad in [true,false] {
            for dpi in [96,120,144,192,288] {
                let geometry=BezelGeometry::new(ipad,dpi);
                assert_eq!(geometry.outer_radius,geometry.aperture_inset()+geometry.inner_radius);
                assert_eq!(geometry.video_clip_radius(),geometry.cover+geometry.inner_radius);
                assert!(geometry.outer_radius>geometry.inner_radius);
                let skin=layout(620,940,ipad,dpi);
                let bgra=render_bezel(skin,false).unwrap();
                let x=geometry.aperture_inset()+geometry.inner_radius;
                // At the upper-left arc's common centre, the inner aperture
                // is fully transparent. Just outside its radius, the rim
                // remains dark (no cyan/white native decoration).
                let alpha=|x:i32,y:i32|
                    bgra[((y*skin.width+x)*4+3) as usize];
                assert_eq!(alpha(x,x),0);
                assert_eq!(alpha(geometry.aperture_inset()+2,
                    geometry.aperture_inset()+2),255);
            }
        }
    }
    #[test]
    fn ipad_clip_scales_the_smaller_radius_to_receiver_units(){
        let scale=RegionScale{physical:(1272,1830),receiver:(848,1220)};
        let geometry=BezelGeometry::new(true,96);
        assert_eq!((geometry.inner_radius,geometry.outer_radius),(16,32));
        let rect=scale.rect(0,0,1273,1831);
        assert_eq!((rect.left,rect.top,rect.right,rect.bottom),(0,0,849,1221));
        assert_eq!(scale.diameter(geometry.video_clip_radius()),(26,26));
    }
    #[test]
    fn dpi_unaware_video_fills_the_original_aperture_and_clips_all_four_corners(){
        use windows::Win32::UI::HiDpi::DPI_AWARENESS_CONTEXT_UNAWARE;
        let _physical_dpi=ThreadDpi::enter(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2).unwrap();
        let class:Vec<u16>="STATIC".encode_utf16().chain(Some(0)).collect();
        let raw={
            let _receiver_dpi=ThreadDpi::enter(DPI_AWARENESS_CONTEXT_UNAWARE).unwrap();
            unsafe{CreateWindowExW(0,class.as_ptr(),ptr::null(),WS_POPUP,
                100,80,848,1220,ptr::null_mut(),ptr::null_mut(),ptr::null_mut(),ptr::null_mut())}
        };
        let _window=WindowOwner(NonNull::new(raw).expect("DPI-unaware video test HWND"));
        let hwnd=HWND(raw);
        let mut bounds=RECT::default();
        unsafe{GetWindowRect(hwnd,&mut bounds)}.unwrap();
        let video=VideoClient{point:Point{x:bounds.left,y:bounds.top},
            width:bounds.right-bounds.left,height:bounds.bottom-bounds.top};
        let clip=clip_signature(hwnd,video,true,96).unwrap();
        assert_eq!(clip.scale.receiver,(848,1220));
        apply_video_corner_clip(hwnd,video,clip).unwrap();
        assert!(native_clip_intact(hwnd,video,clip));
        let rgn=unsafe{CreateRectRgn(0,0,0,0)};
        assert!(!rgn.0.is_null());
        assert!(unsafe{GetWindowRgn(hwnd,rgn)}.0>=2);
        let skin=target_layout(video,"device-ipad",96).unwrap();
        let pixels=render_bezel(skin,false).unwrap();
        let inset=bezel_inset(skin.ipad,skin.dpi);
        let geometry=BezelGeometry::new(true,skin.dpi);
        let cover=geometry.cover as f32;
        let mut leaking_pixels=0;
        let mut exposed_square_corners=0;
        for y in 0..48{
            for x in 0..48{
                for (x,y) in [(x,y),(video.width-1-x,y),
                    (x,video.height-1-y),(video.width-1-x,video.height-1-y)]{
                    let aperture=round_coverage(x as f32+0.5,y as f32+0.5,
                        cover,cover,video.width as f32-2.0*cover,video.height as f32-2.0*cover,
                        geometry.inner_radius as f32);
                    let alpha=pixels[(((y+inset)*skin.width+x+inset)*4+3) as usize];
                    let has_video=unsafe{PtInRegion(rgn,x,y)}.as_bool();
                    if aperture>0.0 && alpha<255 && !has_video {leaking_pixels+=1;}
                    if x.min(video.width-1-x)<=3 && y.min(video.height-1-y)<=3 && has_video {
                        exposed_square_corners+=1;
                    }
                }
            }
        }
        let _=unsafe{GdiDeleteObject(HGDIOBJ(rgn.0))};
        assert_eq!(leaking_pixels,0,"physical region {:?}, receiver {:?}",
            clip.scale.physical,clip.scale.receiver);
        assert_eq!(exposed_square_corners,0);
        clear_video_corner_clip(hwnd);
        assert!(!native_clip_intact(hwnd,video,clip));
        apply_video_corner_clip(hwnd,video,clip).unwrap();
        assert!(native_clip_intact(hwnd,video,clip));
    }
    #[test]
    fn native_win32_gdi_region_clips_all_four_square_video_corners(){
        let _physical_dpi=ThreadDpi::enter(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2).unwrap();
        let class:Vec<u16>="STATIC".encode_utf16().chain(Some(0)).collect();
        let title:Vec<u16>="ios rounded test".encode_utf16().chain(Some(0)).collect();
        let raw=unsafe{CreateWindowExW(0,class.as_ptr(),title.as_ptr(),
            WS_POPUP,100,80,320,480,ptr::null_mut(),ptr::null_mut(),
            ptr::null_mut(),ptr::null_mut())};
        let Some(raw)=NonNull::new(raw) else {panic!("Cannot allocate native test HWND")};
        let hwnd=HWND(raw.as_ptr());
        let video=VideoClient{point:Point{x:100,y:80},width:320,height:480};
        let clip=clip_signature(hwnd,video,true,96).unwrap();
        let result=apply_video_corner_clip(hwnd,video,clip);
        if result.is_ok(){
            assert!(native_clip_intact(hwnd,video,clip));
            let rgn=unsafe{CreateRectRgn(0,0,0,0)};
            assert!(!rgn.0.is_null());
            let actual=unsafe{GetWindowRgn(hwnd,rgn)};
            assert!(actual.0>=2);
            for (x,y) in [(0,0),(319,0),(0,479),(319,479)]{
                assert!(!unsafe{PtInRegion(rgn,x,y)}.as_bool(),
                    "native corner ({x},{y}) must remain transparent");
            }
            // Straight-side resize hit regions remain valid, but the old
            // complete HWND nonclient border that leaked a white arc is gone.
            assert!(unsafe{PtInRegion(rgn,0,240)}.as_bool());
            let _=unsafe{GdiDeleteObject(HGDIOBJ(rgn.0))};
        }
        clear_video_corner_clip(hwnd);
        unsafe{DestroyWindow(raw.as_ptr());}
        result.unwrap();
    }
    #[test]
    fn native_dwm_border_and_backdrop_suppression_is_per_window_and_restorable(){
        assert_eq!(DWMWA_WINDOW_CORNER_PREFERENCE,33);
        assert_eq!(DWMWA_BORDER_COLOR,34);
        assert_eq!(DWMWA_COLOR_NONE,0xFFFF_FFFE);
        let class:Vec<u16>="STATIC".encode_utf16().chain(Some(0)).collect();
        let title:Vec<u16>="ios dwm smoke".encode_utf16().chain(Some(0)).collect();
        let raw=unsafe{CreateWindowExW(0,class.as_ptr(),title.as_ptr(),
            WS_POPUP,100,80,320,480,ptr::null_mut(),ptr::null_mut(),
            ptr::null_mut(),ptr::null_mut())};
        let Some(raw)=NonNull::new(raw) else{panic!("Cannot allocate DWM test HWND")};
        let hwnd=HWND(raw.as_ptr());
        let before=dwm_read_u32(hwnd,DWMWA_BORDER_COLOR);
        let has_backdrop=dwm_write_u32(hwnd,DWMWA_SYSTEMBACKDROP_TYPE,DWMSBT_MAINWINDOW);
        let backdrop_before=dwm_read_u32(hwnd,DWMWA_SYSTEMBACKDROP_TYPE);
        let override_state=ReceiverDwmStyle::attach(hwnd);
        if override_state.wrote_border && before.is_some() {
            assert_eq!(dwm_read_u32(hwnd,DWMWA_BORDER_COLOR),
                Some(DWMWA_COLOR_NONE));
        }
        if has_backdrop && backdrop_before.is_some(){
            assert_eq!(dwm_read_u32(hwnd,DWMWA_SYSTEMBACKDROP_TYPE),Some(DWMSBT_NONE));
        }
        override_state.restore();
        if let Some(original)=before {
            assert_eq!(dwm_read_u32(hwnd,DWMWA_BORDER_COLOR),Some(original));
        }
        if has_backdrop && backdrop_before.is_some(){
            assert_eq!(dwm_read_u32(hwnd,DWMWA_SYSTEMBACKDROP_TYPE),backdrop_before);
        }
        unsafe{DestroyWindow(raw.as_ptr());}
    }
    #[test]
    fn target_geometry_tracks_physical_video_client_even_on_negative_monitors(){
        let input=VideoClient{
            point:Point{x:-1200,y:400},width:700,height:1000
        };
        let expected=target_layout(input,"device-ipad",144).unwrap();
        assert_eq!(expected.point,Point{x:-1218,y:382});
        assert_eq!((expected.width,expected.height),(736,1036));
    }
    #[test]
    fn location_hook_matches_video_client_and_rejects_stale_resizes(){
        let initial=VideoClient{point:Point{x:-1200,y:400},width:700,height:1000};
        let inset=bezel_inset(true,144);
        assert_eq!(moved_video_target(initial,inset,700,1000),
            Some(Point{x:-1218,y:382}));
        let moved=VideoClient{point:Point{x:300,y:0},..initial};
        assert_eq!(moved_video_target(moved,inset,700,1000),
            Some(Point{x:282,y:-18}));
        assert_eq!(moved_video_target(moved,inset,702,1000),None);
    }
}
