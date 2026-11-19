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
    sync::{atomic::{AtomicBool, AtomicI32, AtomicIsize, Ordering}, Arc, Mutex, OnceLock},
    thread::{self, JoinHandle},
    time::{Duration,Instant},
};
use tauri::{AppHandle, Manager};
use windows::Win32::Foundation::{HWND, POINT, RECT};
use windows::Win32::Graphics::Gdi::{
    ClientToScreen, GetMonitorInfoW, MonitorFromWindow, MONITORINFO,
    MONITOR_DEFAULTTONEAREST, CreateRectRgn,CreateRoundRectRgn,
    CombineRgn,SetWindowRgn,DeleteObject,HGDIOBJ,RGN_DIFF,RGN_OR,
    GetWindowRgn,PtInRegion,
};
use windows::Win32::UI::HiDpi::GetDpiForWindow;
use windows::Win32::UI::WindowsAndMessaging::{
    GetClientRect, GetWindowRect, IsIconic, IsWindowVisible,
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
const FOLLOW_MS:u64=16;
// Reuse Android's verified native bezel overscan concept: extend the opaque
// iPad lip a few physical pixels OVER the SDL video, not merely up to it.
// This removes transparent wallpaper leaks during native window movement.
const VIDEO_SEAM_COVER_LOGICAL:i32=crate::frame_layout::BEZEL_CLIP_OVERSCAN_PX+3;
// Android's interactive drag updates SDL + frame in a single native batch.
// A separate AirPlay/SDL process doesn't expose that drag lock. Instead,
// listen to its HWND location-change event and move the border immediately,
// rather than waiting for the 12ms periodic health follower.
const EVENT_OBJECT_LOCATIONCHANGE:u32=0x800B;
const OBJID_WINDOW:i32=0;
const CHILDID_SELF:i32=0;
const WINEVENT_OUTOFCONTEXT:u32=0;
const SWP_NOSIZE:u32=0x0001;
const SWP_NOZORDER:u32=0x0004;
const SWP_NOACTIVATE:u32=0x0010;
static HOOK_VIDEO:AtomicIsize=AtomicIsize::new(0);
static HOOK_BEZEL:AtomicIsize=AtomicIsize::new(0);
static HOOK_INSET:AtomicI32=AtomicI32::new(0);
static HOOK_VIDEO_W:AtomicI32=AtomicI32::new(0);
static HOOK_VIDEO_H:AtomicI32=AtomicI32::new(0);
static HOOK_READY:AtomicBool=AtomicBool::new(false);

#[repr(C)]
#[derive(Clone,Copy,Default,Debug,PartialEq,Eq)]
struct Point{x:i32,y:i32}
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

#[derive(Clone,Copy,Debug,PartialEq,Eq)]
struct VideoClient{point:Point,width:i32,height:i32}
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
struct CaseLayout{point:Point,width:i32,height:i32,ipad:bool,dpi:u32}

fn video_client(hwnd:HWND)->Option<VideoClient>{
    if unsafe{IsIconic(hwnd)}.as_bool() ||
        !unsafe{IsWindowVisible(hwnd)}.as_bool(){return None;}
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

fn is_fullscreen(hwnd:HWND)->bool{
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
fn bezel_inset(ipad:bool,dpi:u32)->i32{
    let logical=if ipad{14}else{18};
    ((logical as i64 * i64::from(dpi.clamp(96,384))+48)/96)
        .clamp(10,72) as i32
}
fn seam_cover_px(dpi:u32)->i32 {
    ((VIDEO_SEAM_COVER_LOGICAL as i64*i64::from(dpi.clamp(96,384))+48)/96)
        .clamp(4,16) as i32
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

#[derive(Clone,Copy,Debug,PartialEq,Eq)]
struct NativeVideoClip{
    hwnd:usize,
    width:i32,
    height:i32,
    local_left:i32,
    local_top:i32,
    ipad:bool,
    dpi:u32,
}
fn clip_signature(hwnd:HWND,client:VideoClient,ipad:bool,dpi:u32)
    ->Option<NativeVideoClip>
{
    let mut outer=RECT::default();
    unsafe{GetWindowRect(hwnd,&mut outer)}.ok()?;
    Some(NativeVideoClip{
        hwnd:hwnd.0 as usize,width:client.width,height:client.height,
        local_left:client.point.x.checked_sub(outer.left)?,
        local_top:client.point.y.checked_sub(outer.top)?,
        ipad,dpi,
    })
}
fn clip_shape(client:VideoClient,outer:RECT,ipad:bool,dpi:u32)
    ->Option<(i32,i32,i32,i32,i32)>
{
    let cover=seam_cover_px(dpi);
    let left=client.point.x.checked_sub(outer.left)?.checked_add(cover)?;
    let top=client.point.y.checked_sub(outer.top)?.checked_add(cover)?;
    let right=left.checked_add(client.width-2*cover)?;
    let bottom=top.checked_add(client.height-2*cover)?;
    let radius=((if ipad{28.0}else{40.0})*dpi as f64/96.0)
        .round().clamp(10.0,180.0) as i32;
    (right>left+2*radius && bottom>top+2*radius)
        .then_some((left,top,right,bottom,radius))
}
/// Reuse Android's native GDI clipping instead of exposing the SDL window's
/// rectangular black corners THROUGH the transparent rounded outer case.
/// Preserve the invisible non-client WS_THICKFRAME resize border: remove
/// only the rectangular video CLIENT and add back the rounded video region.
fn apply_video_corner_clip(hwnd:HWND,client:VideoClient,ipad:bool,dpi:u32)
    ->Result<(),&'static str>
{
    let mut bounds=RECT::default();
    unsafe{GetWindowRect(hwnd,&mut bounds)}
        .map_err(|_|"Cannot inspect AirPlay HWND bounds")?;
    let width=bounds.right-bounds.left;
    let height=bounds.bottom-bounds.top;
    if width<=0 || height<=0 || width>MAX_SIDE || height>MAX_SIDE {
        return Err("Invalid AirPlay video HWND bounds");
    }
    let (left,top,right,bottom,r)=clip_shape(client,bounds,ipad,dpi)
        .ok_or("Invalid rounded video aperture")?;
    let full=unsafe{CreateRectRgn(0,0,width+1,height+1)};
    let client_region=unsafe{CreateRectRgn(
        client.point.x-bounds.left,client.point.y-bounds.top,
        client.point.x-bounds.left+client.width+1,
        client.point.y-bounds.top+client.height+1)};
    let rounded=unsafe{CreateRoundRectRgn(
        left,top,right+1,bottom+1,2*r,2*r)};
    let rest=unsafe{CreateRectRgn(0,0,0,0)};
    let result=unsafe{CreateRectRgn(0,0,0,0)};
    let valid=!full.0.is_null() && !client_region.0.is_null() &&
        !rounded.0.is_null() && !rest.0.is_null() && !result.0.is_null()
        && unsafe{CombineRgn(Some(rest),Some(full),Some(client_region),RGN_DIFF)}.0>=1
        && unsafe{CombineRgn(Some(result),Some(rest),Some(rounded),RGN_OR)}.0>=2;
    for region in [full,client_region,rounded,rest]{
        if !region.0.is_null(){let _=unsafe{DeleteObject(HGDIOBJ(region.0))};}
    }
    if !valid {
        if !result.0.is_null(){let _=unsafe{DeleteObject(HGDIOBJ(result.0))};}
        return Err("Cannot construct AirPlay native rounded clip");
    }
    if unsafe{SetWindowRgn(hwnd,Some(result),true)}==0 {
        let _=unsafe{DeleteObject(HGDIOBJ(result.0))};
        return Err("Windows rejected the video rounded clip");
    }
    Ok(())
}
fn native_clip_intact(hwnd:HWND,client:VideoClient,ipad:bool,dpi:u32)->bool{
    let mut bounds=RECT::default();
    if unsafe{GetWindowRect(hwnd,&mut bounds)}.is_err(){return false;}
    let Some((left,top,right,bottom,_))=
        clip_shape(client,bounds,ipad,dpi) else{return false};
    let rgn=unsafe{CreateRectRgn(0,0,0,0)};
    if rgn.0.is_null(){return false;}
    let kind=unsafe{GetWindowRgn(hwnd,rgn)}.0;
    let valid=kind>=2
        && !unsafe{PtInRegion(rgn,left+1,top+1)}.as_bool()
        && !unsafe{PtInRegion(rgn,right-2,bottom-2)}.as_bool()
        && unsafe{PtInRegion(rgn,(left+right)/2,(top+bottom)/2)}.as_bool();
    let _=unsafe{DeleteObject(HGDIOBJ(rgn.0))};
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

// Antialiased signed distance to an exact rounded rectangle in pixel space.
fn round_coverage(px:f32,py:f32,left:f32,top:f32,w:f32,h:f32,r:f32)->f32 {
    let r=r.max(1.0).min(w.min(h)*0.5);
    let dx=(px-(left+w*0.5)).abs()-(w*0.5-r);
    let dy=(py-(top+h*0.5)).abs()-(h*0.5-r);
    let outside=(dx.max(0.0)*dx.max(0.0)+dy.max(0.0)*dy.max(0.0)).sqrt();
    let distance=outside+dx.max(dy).min(0.0)-r;
    (0.5-distance).clamp(0.0,1.0)
}

/// Build alpha-premultiplied silver/graphite ring in pure code. The middle is
/// COMPLETELY transparent (zero BGRA), so no white/opaque WebView rectangles.
fn render_bezel(layout:CaseLayout)->Result<Vec<u8>,&'static str>{
    let w=layout.width;
    let h=layout.height;
    if w<=0 || h<=0 || i64::from(w)*i64::from(h)>MAX_PIXELS {
        return Err("Invalid native bezel surface");
    }
    let dpi=layout.dpi.clamp(96,384);
    let scale=dpi as f32/96.0;
    let inset=bezel_inset(layout.ipad,dpi);
    let inner_inset=inset+seam_cover_px(dpi);
    // Restore #44351521843's iPad Air silhouette and graphite screen lip.
    let radius=(if layout.ipad {40.0}else{54.0})*scale;
    let hole_radius=(if layout.ipad {29.0}else{41.0})*scale;
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
            let outer=round_coverage(xf,yf,0.0,0.0,w as f32,h as f32,radius);
            if outer==0.0{continue;}
            let inner=round_coverage(xf,yf,inner_inset as f32,inner_inset as f32,
                (w-2*inner_inset) as f32,(h-2*inner_inset) as f32,hole_radius);
            let opacity=(outer*(1.0-inner)).clamp(0.0,1.0);
            if opacity==0.0{continue;}
            let min_edge=x.min(y).min(w-1-x).min(h-1-y) as f32;
            // Original iPad Air: #9caeb4 2px anodized silver hairline,
            // #3d454c graphite transition, #090c11 flat matte front.
            // Intentionally avoid the cyan/blue glowing edge of the preview.
            let shine=((w as f32-xf)/w as f32*0.10)+
                ((h as f32-yf)/h as f32*0.035);
            let (r,g,b)=if min_edge<1.8*scale{
                (156.0+10.0*shine,174.0+9.0*shine,180.0+8.0*shine)
            }else if min_edge<3.2*scale{
                (61.0+6.0*shine,69.0+6.0*shine,76.0+6.0*shine)
            }else if min_edge<5.2*scale{
                (21.0,26.0,31.0)
            }else if min_edge>inner_inset as f32-1.7*scale{
                (17.0,22.0,28.0)
            }else{
                (9.0+3.0*shine,12.0+3.0*shine,17.0+3.0*shine)
            };
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
    let landscape=w>h;
    let center=if landscape{
        (w as f32-7.0*scale,h as f32*0.5)
    }else{(w as f32*0.5,7.0*scale)};
    let cr=if layout.ipad{2.8*scale}else{3.2*scale};
    let area=(6.0*scale).ceil() as i32;
    for y in (center.1 as i32-area).max(0)..(center.1 as i32+area).min(h){
        for x in (center.0 as i32-area).max(0)..(center.0 as i32+area).min(w){
            let distance=((x as f32+0.5-center.0).powi(2)+
                (y as f32+0.5-center.1).powi(2)).sqrt();
            let coverage=(cr+0.5-distance).clamp(0.0,1.0);
            if coverage==0.0{continue;}
            let a=(coverage*255.0).round() as u8;
            let pos=((y as usize)*(w as usize)+x as usize)*4;
            let shade=if distance<cr*0.57{(24,42,55)}else{(4,7,12)};
            pixels[pos]=((shade.2 as u16*a as u16)/255) as u8;
            pixels[pos+1]=((shade.1 as u16*a as u16)/255) as u8;
            pixels[pos+2]=((shade.0 as u16*a as u16)/255) as u8;
            pixels[pos+3]=a;
        }
    }
    Ok(pixels)
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
    fn present(&mut self,hwnd:*mut c_void,layout:CaseLayout)
        ->Result<(),&'static str>
    {
        let bgra=render_bezel(layout)?;
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
fn active()->&'static Mutex<Option<NativeWorker>>{
    ACTIVE.get_or_init(||Mutex::new(None))
}
pub fn active_for(pid:u32)->bool{
    active().lock().ok().and_then(|worker|worker.as_ref()
        .map(|w|w.pid==pid && w.alive.load(Ordering::Acquire)))
        .unwrap_or(false)
}
pub fn close(app:&AppHandle){
    let worker=active().lock().ok().and_then(|mut w|w.take());
    if let Some(mut worker)=worker{
        worker.stop.store(true,Ordering::Release);
        if let Some(thread)=worker.thread.take(){let _=thread.join();}
    }
    if let Some(state)=app.try_state::<crate::ios_airplay::IosAirplayState>(){
        state.frame_attached.store(false,Ordering::Release);
        if let Ok(mut error)=state.frame_error.lock(){*error=None;}
    }
}
pub fn start(app:&AppHandle,pid:u32)->Result<(),String>{
    close(app);
    let stop=Arc::new(AtomicBool::new(false));
    let alive=Arc::new(AtomicBool::new(true));
    let stop_worker=stop.clone();
    let alive_worker=alive.clone();
    let app_worker=app.clone();
    let thread=thread::Builder::new().name("ios-native-bezel".into())
        .spawn(move||{
            if let Err(reason)=run_native(app_worker.clone(),pid,&stop_worker){
                if let Some(state)=app_worker.try_state::<crate::ios_airplay::IosAirplayState>(){
                    state.frame_attached.store(false,Ordering::Release);
                    if let Ok(mut error)=state.frame_error.lock(){
                        *error=Some(reason.to_string());
                    }
                }
            }
            alive_worker.store(false,Ordering::Release);
        }).map_err(|e|format!("Cannot start native iOS bezel worker: {e}"))?;
    *active().lock().map_err(|e|e.to_string())?=Some(NativeWorker{
        pid,stop,alive,thread:Some(thread)
    });
    Ok(())
}
fn pump_messages(){
    let mut msg=WinMsg::default();
    while unsafe{PeekMessageW(&mut msg,ptr::null_mut(),0,0,1)}!=0{
        unsafe{TranslateMessage(&msg);DispatchMessageW(&msg);}
    }
}
fn hide(hwnd:*mut c_void,shown:&mut bool){
    HOOK_READY.store(false,Ordering::Release);
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
/// Out-of-context accessibility location event. It executes on the native
/// bezel worker's message pump (no Windows cross-process DLL injection).
/// The only affected HWND is our own click-through native layer.
unsafe extern "system" fn on_video_location_change(
    _hook:*mut c_void,_event:u32,hwnd:*mut c_void,
    object_id:i32,child_id:i32,_thread:u32,_time:u32,
){
    if object_id!=OBJID_WINDOW || child_id!=CHILDID_SELF ||
        !HOOK_READY.load(Ordering::Acquire) ||
        hwnd as isize!=HOOK_VIDEO.load(Ordering::Relaxed){
        return;
    }
    let bezel=HOOK_BEZEL.load(Ordering::Relaxed) as *mut c_void;
    if bezel.is_null(){return;}
    let Some(screen)=video_client(HWND(hwnd)) else{return;};
    let Some(target)=moved_video_target(
        screen,HOOK_INSET.load(Ordering::Relaxed),
        HOOK_VIDEO_W.load(Ordering::Relaxed),
        HOOK_VIDEO_H.load(Ordering::Relaxed),
    )else{return;};
    unsafe{SetWindowPos(bezel,ptr::null_mut(),target.x,target.y,0,0,
        SWP_NOSIZE|SWP_NOZORDER|SWP_NOACTIVATE);}
}
struct NativeLocationHook(*mut c_void);
impl NativeLocationHook{
    fn install(pid:u32)->Self{
        let hook=unsafe{SetWinEventHook(
            EVENT_OBJECT_LOCATIONCHANGE,EVENT_OBJECT_LOCATIONCHANGE,
            ptr::null_mut(),Some(on_video_location_change),
            pid,0,WINEVENT_OUTOFCONTEXT
        )};
        Self(hook)
    }
}
impl Drop for NativeLocationHook{
    fn drop(&mut self){
        HOOK_READY.store(false,Ordering::Release);
        HOOK_VIDEO.store(0,Ordering::Relaxed);
        HOOK_BEZEL.store(0,Ordering::Relaxed);
        if !self.0.is_null(){unsafe{UnhookWinEvent(self.0);}}
    }
}
fn run_native(app:AppHandle,pid:u32,stop:&AtomicBool)
    ->Result<(),&'static str>
{
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
    // Exclusive thread ownership ensures DestroyWindow always runs on the
    // same thread that created it, also for a failed rendering attempt.
    struct WindowOwner(NonNull<c_void>);
    impl Drop for WindowOwner{
        fn drop(&mut self){unsafe{DestroyWindow(self.0.as_ptr());}}
    }
    let _window=WindowOwner(hwnd);
    let _location_hook=NativeLocationHook::install(pid);
    let mut surface=Surface::create()?;
    let mut controls=crate::ios_window_controls::NativeControls::create().ok();
    let mut previous_skin:Option<(i32,i32,bool,u32)>=None;
    let mut applied_video_clip:Option<NativeVideoClip>=None;
    let mut clip_checked_at=Instant::now()-Duration::from_secs(1);
    let mut shown=false;
    let mut applied=false;
    while !stop.load(Ordering::Acquire){
        pump_messages();
        match crate::ios_window_controls::NativeControls::action(){
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
        if crate::ios_airplay::active_receiver_pid(&state)!=Some(pid){break;}
        let style=state.window_style.lock().ok().map(|s|s.clone())
            .unwrap_or_default();
        let video=crate::commands::find_window_by_pid(pid);
        let Some(video)=video else{
            if let Some(control)=controls.as_mut(){control.hide();}
            hide(hwnd.as_ptr(),&mut shown);
            state.frame_attached.store(false,Ordering::Release);
            thread::sleep(Duration::from_millis(50));
            continue;
        };
        let Some(screen)=video_client(video) else{
            if let Some(control)=controls.as_mut(){control.hide();}
            hide(hwnd.as_ptr(),&mut shown);
            state.frame_attached.store(false,Ordering::Release);
            thread::sleep(Duration::from_millis(40));
            continue;
        };
        // Two native buttons are available even in "clean" mode; control
        // only this AirPlay receiver, never the Android mirror/main GUI.
        if let Some(control)=controls.as_mut(){
            control.update_hover(video,screen.point.x,screen.point.y,screen.width);
        }
        if !style.starts_with("device-") || is_fullscreen(video){
            clear_clip_if_still_owned(pid,&mut applied_video_clip);
            hide(hwnd.as_ptr(),&mut shown);
            state.frame_attached.store(false,Ordering::Release);
            thread::sleep(Duration::from_millis(40));
            continue;
        }
        let dpi=unsafe{GetDpiForWindow(video)}.clamp(96,384);
        let Some(layout)=target_layout(screen,&style,dpi) else{
            hide(hwnd.as_ptr(),&mut shown);
            state.frame_attached.store(false,Ordering::Release);
            set_error(&app,Some("视频窗口过大，已安全隐藏真机边框"));
            thread::sleep(Duration::from_millis(80));
            continue;
        };
        let desired_clip=clip_signature(video,screen,layout.ipad,dpi);
        let needs_clip=desired_clip.is_some_and(|signature|
            applied_video_clip!=Some(signature) ||
                (clip_checked_at.elapsed()>Duration::from_millis(650) &&
                !native_clip_intact(video,screen,layout.ipad,dpi)));
        if needs_clip {
            if let Err(error)=apply_video_corner_clip(video,screen,layout.ipad,dpi){
                clear_clip_if_still_owned(pid,&mut applied_video_clip);
                hide(hwnd.as_ptr(),&mut shown);
                state.frame_attached.store(false,Ordering::Release);
                set_error(&app,Some(error));
                thread::sleep(Duration::from_millis(200));
                continue;
            }
            applied_video_clip=desired_clip;
        }
        if clip_checked_at.elapsed()>Duration::from_millis(650){
            clip_checked_at=Instant::now();
        }
        let skin=(layout.width,layout.height,layout.ipad,layout.dpi);
        if previous_skin!=Some(skin){
            if let Err(reason)=surface.present(hwnd.as_ptr(),layout){
                hide(hwnd.as_ptr(),&mut shown);
                state.frame_attached.store(false,Ordering::Release);
                set_error(&app,Some(reason));
                thread::sleep(Duration::from_millis(250));
                continue;
            }
            previous_skin=Some(skin);
        }
        // Use native Win32 stacking; position above the SDL HWND in the
        // SAME z-order band rather than creating an always-topmost overlay.
        let location=crate::device_frame::place_ios_companion(
            HWND(hwnd.as_ptr()),video,layout.point.x,layout.point.y,
            layout.width,layout.height);
        if location.is_err(){
            hide(hwnd.as_ptr(),&mut shown);
            state.frame_attached.store(false,Ordering::Release);
            set_error(&app,Some("Windows 拒绝将原生边框放在投屏窗口上方"));
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
            hide(hwnd.as_ptr(),&mut shown);
            state.frame_attached.store(false,Ordering::Release);
            set_error(&app,Some("原生外壳位置验证失败，已隐藏以防止边框分离"));
            thread::sleep(Duration::from_millis(45));
            continue;
        }
        // Transparent centre is guaranteed by BGRA alpha=0, not by WebView
        // CSS, SetParent or DPI-virtualized browser client coordinates.
        show(hwnd.as_ptr(),&mut shown);
        if shown{
            HOOK_READY.store(false,Ordering::Release);
            HOOK_VIDEO.store(video.0 as isize,Ordering::Relaxed);
            HOOK_BEZEL.store(hwnd.as_ptr() as isize,Ordering::Relaxed);
            HOOK_INSET.store(bezel_inset(layout.ipad,layout.dpi),Ordering::Relaxed);
            HOOK_VIDEO_W.store(screen.width,Ordering::Relaxed);
            HOOK_VIDEO_H.store(screen.height,Ordering::Relaxed);
            HOOK_READY.store(true,Ordering::Release);
            state.frame_attached.store(true,Ordering::Release);
            if !applied{set_error(&app,None);applied=true;}
        }
        thread::sleep(Duration::from_millis(FOLLOW_MS));
    }
    clear_clip_if_still_owned(pid,&mut applied_video_clip);
    if let Some(state)=app.try_state::<crate::ios_airplay::IosAirplayState>(){
        state.frame_attached.store(false,Ordering::Release);
    }
    Ok(())
}

#[cfg(test)]
mod tests{
    use super::*;
    fn layout(w:i32,h:i32,ipad:bool,dpi:u32)->CaseLayout{
        CaseLayout{point:Point{x:100,y:150},width:w,height:h,ipad,dpi}
    }
    #[test]
    fn native_layer_does_not_reparent_tauri_or_modify_any_webview(){
        assert!(!active_for(0));
        assert_eq!(bezel_inset(true,96),14);
        assert_eq!(bezel_inset(true,144),21);
        assert_eq!(bezel_inset(false,192),36);
        assert!(choose_ipad("device-auto",1024,1366));
        assert!(!choose_ipad("device-auto",500,1120));
    }
    #[test]
    fn hollow_rim_preserves_entire_unscaled_video_centre(){
        let view=layout(760,1080,true,96);
        let bgra=render_bezel(view).unwrap();
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
        let bgra=render_bezel(skin).unwrap();
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
        let bgra=render_bezel(skin).unwrap();
        let p=((skin.height/2*skin.width+1)*4) as usize;
        let (blue,green,red,alpha)=(bgra[p],bgra[p+1],bgra[p+2],bgra[p+3]);
        assert_eq!(alpha,255);
        assert!((150..=170).contains(&red));
        assert!((170..=190).contains(&green));
        assert!((176..=194).contains(&blue));
        assert!(blue.saturating_sub(red)<32);
    }
    #[test]
    fn rounded_outer_corner_is_antialiased_and_zero_outside(){
        let bgra=render_bezel(layout(420,820,false,96)).unwrap();
        assert_eq!(&bgra[..4],&[0,0,0,0]);
        assert_eq!(bgra[((2*420+210)*4+3) as usize]>0,true);
        assert!(render_bezel(layout(6000,6000,true,96)).is_err());
    }
    #[test]
    fn each_inner_round_corner_is_covered_not_skipped_as_rectangle(){
        for ipad in [true,false] {
            for dpi in [96,144] {
                let skin=layout(520,880,ipad,dpi);
                let pixels=render_bezel(skin).unwrap();
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
        assert_eq!((left,top,right,bottom),(12,12,704,984));
        assert!(radius>=28);
        let (_,_,_,_,hi_dpi)=clip_shape(video,outer,true,144).unwrap();
        assert!(hi_dpi>radius);
    }
    #[test]
    fn native_win32_gdi_region_clips_all_four_square_video_corners(){
        let class:Vec<u16>="STATIC".encode_utf16().chain(Some(0)).collect();
        let title:Vec<u16>="ios rounded test".encode_utf16().chain(Some(0)).collect();
        let raw=unsafe{CreateWindowExW(0,class.as_ptr(),title.as_ptr(),
            WS_POPUP,100,80,320,480,ptr::null_mut(),ptr::null_mut(),
            ptr::null_mut(),ptr::null_mut())};
        let Some(raw)=NonNull::new(raw) else {panic!("Cannot allocate native test HWND")};
        let hwnd=HWND(raw.as_ptr());
        let video=VideoClient{point:Point{x:100,y:80},width:320,height:480};
        let result=apply_video_corner_clip(hwnd,video,true,96);
        if result.is_ok(){assert!(native_clip_intact(hwnd,video,true,96));}
        clear_video_corner_clip(hwnd);
        unsafe{DestroyWindow(raw.as_ptr());}
        result.unwrap();
    }
    #[test]
    fn target_geometry_tracks_physical_video_client_even_on_negative_monitors(){
        let input=VideoClient{
            point:Point{x:-1200,y:400},width:700,height:1000
        };
        let expected=target_layout(input,"device-ipad",144).unwrap();
        assert_eq!(expected.point,Point{x:-1221,y:379});
        assert_eq!((expected.width,expected.height),(742,1042));
    }
    #[test]
    fn location_hook_matches_video_client_and_rejects_stale_resizes(){
        let initial=VideoClient{point:Point{x:-1200,y:400},width:700,height:1000};
        let inset=bezel_inset(true,144);
        assert_eq!(moved_video_target(initial,inset,700,1000),
            Some(Point{x:-1221,y:379}));
        let moved=VideoClient{point:Point{x:300,y:0},..initial};
        assert_eq!(moved_video_target(moved,inset,700,1000),
            Some(Point{x:279,y:-21}));
        assert_eq!(moved_video_target(moved,inset,702,1000),None);
    }
}
