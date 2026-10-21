//! Opt-in Windows native live magnifier. Unlike Android `adb screencap`,
//! Magnification.dll reads the continuously composited *actual SDL video*
//! from the desktop and renders it using Windows' native magnifier control.
//! Each lens owns a mouse-through HWND and an OS message pump. No pixels are
//! saved, transmitted, encoded to PNG or decoded in WebView2.
//
//! The native Magnification control works on the SCREEN, so protected video,
//! secure desktops, remote sessions and some GPUs can refuse magnification.
//! Fail explicitly rather than showing a frozen ADB image as "real time".
#![cfg(target_os = "windows")]

use std::{
    ffi::c_void,
    ptr::{null, null_mut},
    sync::{atomic::{AtomicI32, AtomicU32, AtomicU64, Ordering}, mpsc::{sync_channel, Receiver, SyncSender, TrySendError}, Arc, Mutex, OnceLock},
    thread::{self, JoinHandle},
    time::Duration,
};

type RawHwnd = *mut c_void;
use crate::hq_bridge_windows::HqMappingOwner;
use crate::hq_cpu_lens_windows::HqDecodedCpuLens;
use crate::hq_geometry::ScreenViewport;
const EX_LAYERED: u32 = 0x0008_0000;
const EX_TRANSPARENT: u32 = 0x0000_0020;
const EX_TOOLWINDOW: u32 = 0x0000_0080;
const EX_NOACTIVATE: u32 = 0x0800_0000;
const EX_TOPMOST: u32 = 0x0000_0008;
const STYLE_POPUP: u32 = 0x8000_0000;
const STYLE_CHILD: u32 = 0x4000_0000;
const STYLE_VISIBLE: u32 = 0x1000_0000;
const LWA_ALPHA: u32 = 2;
const POSITION_SHOW: u32 = 0x0040 | 0x0010;
const POSITION_CHILD: u32 = 0x0004 | 0x0010;
// Creator-preferred zoom is 2.35×. Keep this ratio stable; reducing zoom
// is not a fix for the genuine information loss caused by magnifying an
// already downscaled SDL desktop window. Ctrl+wheel remains an explicit user
// choice. True 2.35× high-definition must read decoded AVFrame video BEFORE
// scrcpy downsamples to SDL (see MIRROCREATOR_HQ_RENDERING_CN.md).
const DEFAULT_FACTOR: f32 = 2.35;
const FACTOR_MIN: f32 = 1.05;
const FACTOR_MAX: f32 = 3.0;
const DEFAULT_DIAMETER_RATIO: f32 = 0.50;
const DIAMETER_MIN: f32 = 0.28;
const DIAMETER_MAX: f32 = 0.90;
const PREVIEW_MIN: i32 = 110;
const PREVIEW_MAX: i32 = 720;
const WM_MOUSEWHEEL: usize = 0x020A;
const WH_MOUSE_LL: i32 = 14;
const VK_CONTROL: i32 = 0x11;

static WHEEL_SIZE_DELTA:AtomicI32=AtomicI32::new(0);
static WHEEL_ZOOM_DELTA:AtomicI32=AtomicI32::new(0);
static WHEEL_LAST_X:AtomicI32=AtomicI32::new(i32::MIN);
static WHEEL_LAST_Y:AtomicI32=AtomicI32::new(i32::MIN);
static WHEEL_ACTIVE:AtomicI32=AtomicI32::new(0);
static WHEEL_PHONE_X:AtomicI32=AtomicI32::new(0);
static WHEEL_PHONE_Y:AtomicI32=AtomicI32::new(0);
static WHEEL_PHONE_W:AtomicI32=AtomicI32::new(0);
static WHEEL_PHONE_H:AtomicI32=AtomicI32::new(0);

fn inside_active_phone_rect(px:i32,py:i32,x:i32,y:i32,w:i32,h:i32)->bool {
    w>0 && h>0 && px>=x && py>=y &&
        px.saturating_sub(x)<w && py.saturating_sub(y)<h
}
fn wheel_target_contains(px:i32,py:i32)->bool {
    if WHEEL_ACTIVE.load(Ordering::Relaxed)!=1 {return false;}
    let x=WHEEL_PHONE_X.load(Ordering::Relaxed);
    let y=WHEEL_PHONE_Y.load(Ordering::Relaxed);
    let w=WHEEL_PHONE_W.load(Ordering::Relaxed);
    let h=WHEEL_PHONE_H.load(Ordering::Relaxed);
    inside_active_phone_rect(px,py,x,y,w,h)
}
fn set_wheel_target(view:ViewUpdate) {
    // The low-level hook must run fast: only atomic snapshots, never locks
    // or IPC on its hot path.
    WHEEL_ACTIVE.store(0,Ordering::SeqCst);
    WHEEL_PHONE_X.store(view.x,Ordering::Relaxed);
    WHEEL_PHONE_Y.store(view.y,Ordering::Relaxed);
    WHEEL_PHONE_W.store(view.width,Ordering::Relaxed);
    WHEEL_PHONE_H.store(view.height,Ordering::Relaxed);
    WHEEL_ACTIVE.store(i32::from(view.active && view.inside()),Ordering::SeqCst);
}


#[repr(C)]
#[derive(Clone, Copy, Default, Debug)]
struct Rect { left: i32, top: i32, right: i32, bottom: i32 }
#[repr(C)]
struct MagTransform { v: [[f32;3];3] }
#[repr(C)]
#[derive(Clone, Copy, Default)]
struct Point { x:i32, y:i32 }
#[repr(C)]
#[derive(Clone, Copy)]
struct Msg {
    hwnd:RawHwnd, message:u32, wparam:usize, lparam:isize,
    time:u32, point:Point, private:u32,
}
impl Default for Msg {
    fn default()->Self {unsafe{std::mem::zeroed()}}
}


#[repr(C)]
struct LowLevelMouse {
    point:Point, mouse_data:u32, flags:u32, time:u32, extra_info:usize,
}

type HookProc = unsafe extern "system" fn(i32,usize,isize)->isize;

/// Installed ONLY while the live lens is active. A passive hook forwards
/// every wheel event to the application/SDL unchanged.
unsafe extern "system" fn mouse_wheel_hook(code:i32,message:usize,data:isize)->isize {
    if code>=0 && message==WM_MOUSEWHEEL && data!=0 {
        let wheel=&*(data as *const LowLevelMouse);
        let delta=(wheel.mouse_data>>16) as i16 as i32;
        if delta!=0 && wheel_target_contains(wheel.point.x,wheel.point.y) {
            WHEEL_LAST_X.store(wheel.point.x,Ordering::Relaxed);
            WHEEL_LAST_Y.store(wheel.point.y,Ordering::Relaxed);
            let ctrl=(GetAsyncKeyState(VK_CONTROL) as u16 & 0x8000)!=0;
            if ctrl {
                WHEEL_ZOOM_DELTA.fetch_add(delta,Ordering::Relaxed);
            } else {
                WHEEL_SIZE_DELTA.fetch_add(delta,Ordering::Relaxed);
            }
            // Consume ONLY wheel events on this foreground phone while the
            // real-time lens is active. Otherwise swipe-driven apps advance
            // videos rather than modifying the magnifier.
            return 1;
        }
    }
    CallNextHookEx(null_mut(),code,message,data)
}

struct ScopedWheelHook(RawHwnd);
impl ScopedWheelHook {
    fn install()->Result<Self,String> {
        WHEEL_SIZE_DELTA.store(0,Ordering::Relaxed);
        WHEEL_ZOOM_DELTA.store(0,Ordering::Relaxed);
        let module=unsafe {GetModuleHandleW(null())};
        let handle=unsafe {SetWindowsHookExW(WH_MOUSE_LL,Some(mouse_wheel_hook),module,0)};
        if handle.is_null() {
            Err("Cannot listen to the mouse wheel for the live magnifier".into())
        } else {
            Ok(Self(handle))
        }
    }
}
impl Drop for ScopedWheelHook {
    fn drop(&mut self) {
        WHEEL_ACTIVE.store(0,Ordering::SeqCst);
        unsafe {UnhookWindowsHookEx(self.0);}
        WHEEL_SIZE_DELTA.store(0,Ordering::Relaxed);
        WHEEL_ZOOM_DELTA.store(0,Ordering::Relaxed);
    }
}

#[link(name = "Magnification")]
unsafe extern "system" {
    fn MagInitialize() -> i32;
    fn MagUninitialize() -> i32;
    fn MagSetWindowTransform(hwnd:RawHwnd, transform:*const MagTransform)->i32;
    fn MagSetWindowSource(hwnd:RawHwnd, source:Rect)->i32;
    fn MagSetWindowFilterList(hwnd:RawHwnd, filter_mode:u32, count:i32, windows:*const RawHwnd)->i32;
}
#[link(name = "user32")]
unsafe extern "system" {
    fn CreateWindowExW(ex:u32,class:*const u16,title:*const u16,style:u32,
        x:i32,y:i32,width:i32,height:i32,parent:RawHwnd,menu:RawHwnd,
        instance:RawHwnd,extra:*mut c_void)->RawHwnd;
    fn SetLayeredWindowAttributes(hwnd:RawHwnd,color_key:u32,alpha:u8,flags:u32)->i32;
    fn SetWindowPos(hwnd:RawHwnd,insert_after:RawHwnd,x:i32,y:i32,width:i32,height:i32,flags:u32)->i32;
    fn SetWindowRgn(hwnd:RawHwnd,region:RawHwnd,redraw:i32)->i32;
    fn ShowWindow(hwnd:RawHwnd,command:i32)->i32;
    fn DestroyWindow(hwnd:RawHwnd)->i32;
    fn InvalidateRect(hwnd:RawHwnd,rect:*const Rect,erase:i32)->i32;
    fn PeekMessageW(msg:*mut Msg, hwnd:RawHwnd,low:u32,high:u32,remove:u32)->i32;
    fn TranslateMessage(msg:*const Msg)->i32;
    fn DispatchMessageW(msg:*const Msg)->isize;
    fn GetAsyncKeyState(key:i32)->i16;
    fn SetWindowsHookExW(id:i32,callback:Option<HookProc>,instance:RawHwnd,thread:u32)->RawHwnd;
    fn UnhookWindowsHookEx(hook:RawHwnd)->i32;
    fn CallNextHookEx(hook:RawHwnd,code:i32,message:usize,data:isize)->isize;
}
#[link(name = "kernel32")]
unsafe extern "system" {
    fn GetModuleHandleW(name:*const u16)->RawHwnd;
}

#[link(name = "gdi32")]
unsafe extern "system" {
    fn CreateEllipticRgn(left:i32,top:i32,right:i32,bottom:i32)->RawHwnd;
    fn DeleteObject(object:RawHwnd)->i32;
}

fn wide(s:&str)->Vec<u16> {s.encode_utf16().chain(Some(0)).collect()}

#[derive(Clone,Copy,Debug)]
pub struct ViewUpdate {
    pub x:i32, pub y:i32, pub width:i32, pub height:i32,
    pub cursor_x:i32, pub cursor_y:i32, pub active:bool,
    pub frame_hwnd:usize, pub controls_hwnd:usize,
}
impl ViewUpdate {
    pub fn inside(&self)->bool {
        self.width>=80 && self.height>=80 &&
        self.cursor_x>=self.x && self.cursor_x<self.x+self.width &&
        self.cursor_y>=self.y && self.cursor_y<self.y+self.height
    }
}
#[derive(Clone,Copy,Debug)]
struct LensSettings { diameter_ratio:f32, zoom:f32 }
impl Default for LensSettings {
    fn default()->Self {
        Self{diameter_ratio:DEFAULT_DIAMETER_RATIO,zoom:DEFAULT_FACTOR}
    }
}
impl LensSettings {
    fn adjust_wheel(&mut self, delta:i32, ctrl:bool)->bool {
        if delta==0 {return false;}
        // Supports both ordinary 120-delta wheels and precision touchpads.
        let steps=(delta as f32 / 120.0).clamp(-6.0,6.0);
        if ctrl {
            let next=(self.zoom+steps*0.18).clamp(FACTOR_MIN,FACTOR_MAX);
            if (next-self.zoom).abs()<0.001 {return false;}
            self.zoom=next;
        } else {
            let next=(self.diameter_ratio+steps*0.07).clamp(DIAMETER_MIN,DIAMETER_MAX);
            if (next-self.diameter_ratio).abs()<0.001 {return false;}
            self.diameter_ratio=next;
        }
        true
    }
}

#[derive(Clone,Copy,Debug)]
struct Geometry { x:i32, y:i32, size:i32, source:Rect }
fn lens_geometry(view:ViewUpdate,settings:LensSettings)->Option<Geometry> {
    if !view.active || !view.inside() { return None; }
    let size=((view.width.min(view.height) as f32)*settings.diameter_ratio).round() as i32;
    let size=size.clamp(PREVIEW_MIN,PREVIEW_MAX)
        .min(view.width.saturating_sub(8))
        .min(view.height.saturating_sub(8));
    if size<60 {return None;}
    let x=(view.cursor_x-size/2).clamp(view.x+4,view.x+view.width-size-4);
    let y=(view.cursor_y-size/2).clamp(view.y+4,view.y+view.height-size-4);
    // Keep the source CENTERED on the actual Android pointer even when the
    // visible lens is shifted inward to avoid clipping edge applications.
    let src=(size as f32/settings.zoom).round() as i32;
    let src=src.max(2);
    Some(Geometry {
        x,y,size,
        source:Rect {
            left:view.cursor_x-src/2, top:view.cursor_y-src/2,
            right:view.cursor_x-src/2+src, bottom:view.cursor_y-src/2+src,
        },
    })
}

struct Lens { host:RawHwnd, magnifier:RawHwnd, visible:bool, size:i32, exclusion:(usize,usize), settings:LensSettings, view:Option<ViewUpdate> }
impl Lens {
    fn create()->Result<Self,String> {
        unsafe {
            if MagInitialize()==0 {
                return Err("Windows Magnification API could not initialize on this desktop".into());
            }
            let class=wide("STATIC");
            let title=wide("MirroLeap native live lens");
            let host=CreateWindowExW(
                EX_LAYERED|EX_TRANSPARENT|EX_TOOLWINDOW|EX_NOACTIVATE|EX_TOPMOST,
                class.as_ptr(),title.as_ptr(),STYLE_POPUP,
                0,0,PREVIEW_MIN,PREVIEW_MIN,null_mut(),null_mut(),null_mut(),null_mut());
            if host.is_null() {
                MagUninitialize();
                return Err("Windows could not create a click-through live lens window".into());
            }
            if SetLayeredWindowAttributes(host,0,255,LWA_ALPHA)==0 {
                DestroyWindow(host);
                MagUninitialize();
                return Err("Windows could not make the live lens mouse-through".into());
            }
            let mag_class=wide("Magnifier");
            let control=CreateWindowExW(0,mag_class.as_ptr(),null(),
                STYLE_CHILD|STYLE_VISIBLE,0,0,PREVIEW_MIN,PREVIEW_MIN,
                host,null_mut(),null_mut(),null_mut());
            if control.is_null() {
                DestroyWindow(host);
                MagUninitialize();
                return Err("Windows Magnification control is unavailable on this system".into());
            }
            let zoom=DEFAULT_FACTOR;
            let transform=MagTransform {v:[
                [zoom,0.,0.],
                [0.,zoom,0.],
                [0.,0.,1.],
            ]};
            if MagSetWindowTransform(control,&transform)==0 {
                DestroyWindow(control);
                DestroyWindow(host);
                MagUninitialize();
                return Err("Windows could not enable live graphics magnification".into());
            }
            Ok(Self{host,magnifier:control,visible:false,size:0,exclusion:(0,0),settings:LensSettings::default(),view:None})
        }
    }
    fn resize(&mut self,size:i32) {
        if self.size==size {return;}
        unsafe {
            let circle=CreateEllipticRgn(0,0,size,size);
            if !circle.is_null() && SetWindowRgn(self.host,circle,1)==0 {
                DeleteObject(circle);
            }
            SetWindowPos(self.magnifier,null_mut(),0,0,size,size,POSITION_CHILD);
        }
        self.size=size;
    }
    fn set_visible(&mut self,value:bool) {
        if !value { WHEEL_ACTIVE.store(0,Ordering::SeqCst); }
        if self.visible==value {return;}
        if !value {unsafe {ShowWindow(self.host,0);}}
        self.visible=value;
    }
    fn display(&mut self,view:ViewUpdate) {
        self.view=Some(view);
        let Some(target)=lens_geometry(view,self.settings) else {self.set_visible(false);return;};
        self.resize(target.size);
        if self.exclusion!=(view.frame_hwnd,view.controls_hwnd) {
            let windows=[view.frame_hwnd as RawHwnd,view.controls_hwnd as RawHwnd];
            // Exclude translucent bezel and controls from the magnified
            // source. A compositor without selective filters can still show
            // the underlying screen; this is a visual feature, not security.
            unsafe {MagSetWindowFilterList(self.magnifier,0,2,windows.as_ptr());}
            self.exclusion=(view.frame_hwnd,view.controls_hwnd);
        }
        unsafe {
            // DWM produces live updates as the SDL video changes. Refreshing
            // this source is NOT Android screencap or a cached PNG frame.
            if MagSetWindowSource(self.magnifier,target.source)==0 {
                self.set_visible(false);
                return;
            }
            if SetWindowPos(self.host,(-1_isize) as RawHwnd,target.x,target.y,
                target.size,target.size,POSITION_SHOW)==0 {
                self.set_visible(false);
                return;
            }
            self.visible=true;
            // Publish the exact active phone rect only after the magnifier
            // successfully updated and showed its window.
            set_wheel_target(view);
        }
    }
    fn apply_wheel(&mut self,render_legacy:bool) {
        let diameter_delta=WHEEL_SIZE_DELTA.swap(0,Ordering::SeqCst);
        let zoom_delta=WHEEL_ZOOM_DELTA.swap(0,Ordering::SeqCst);
        if diameter_delta==0 && zoom_delta==0 {return;}
        let Some(view)=self.view else {return;};
        let wx=WHEEL_LAST_X.load(Ordering::SeqCst);
        let wy=WHEEL_LAST_Y.load(Ordering::SeqCst);
        // The low-level wheel observer runs system-wide *only while live
        // magnification is enabled*, but only events over this active phone
        // are permitted to modify its presentation state.
        if !view.active || !view.inside() ||
            wx<view.x || wx>=view.x+view.width ||
            wy<view.y || wy>=view.y+view.height {return;}

        let changed_diameter=self.settings.adjust_wheel(diameter_delta,false);
        let previous_zoom=self.settings.zoom;
        let changed_zoom=self.settings.adjust_wheel(zoom_delta,true);
        let mut zoom_applied=false;
        if changed_zoom {
            let z=self.settings.zoom;
            let transform=MagTransform {v:[
                [z,0.,0.],
                [0.,z,0.],
                [0.,0.,1.],
            ]};
            if unsafe {MagSetWindowTransform(self.magnifier,&transform)}!=0 {
                zoom_applied=true;
            } else {
                // If the GPU rejects a new transform, retain the last working
                // value instead of showing a distorted/stuck source.
                self.settings.zoom=previous_zoom;
            }
        }
        if (changed_diameter || zoom_applied) && render_legacy {self.display(view);}
    }
    fn refresh(&self) {
        if self.visible { unsafe {InvalidateRect(self.magnifier,null(),0);}}
    }
}
impl Drop for Lens {
    fn drop(&mut self) {
        unsafe {
            ShowWindow(self.host,0);
            DestroyWindow(self.magnifier);
            DestroyWindow(self.host);
            MagUninitialize();
        }
    }
}
enum Command { Update(ViewUpdate) }
struct Session {
    device:String, tx:SyncSender<Command>, thread:JoinHandle<()>,
    // These atomics represent ACTUAL presentation, not mere IPC acceptance.
    mode:Arc<AtomicU32>,source_size:Arc<AtomicU64>,hq_requested:bool,
}
#[derive(serde::Serialize)]
#[serde(rename_all="camelCase")]
pub struct LensSourceStatus {
    pub mode:&'static str, // off, desktop or hq_cpu
    pub hq_requested:bool,
    pub source_width:u32,
    pub source_height:u32,
}
pub fn source_status(device:&str)->LensSourceStatus {
    let unavailable=LensSourceStatus {
        mode:"off",hq_requested:false,source_width:0,source_height:0,
    };
    let Ok(sessions)=current().try_lock() else {return unavailable;};
    let Some(session)=sessions.as_ref().filter(|s|s.device==device) else {
        return unavailable;
    };
    let mode=match session.mode.load(Ordering::Acquire) {
        2=>"hq_cpu",1=>"desktop",_=>"off",
    };
    let source=if mode=="hq_cpu" {session.source_size.load(Ordering::Acquire)}
        else {0};
    LensSourceStatus {
        mode,hq_requested:session.hq_requested,
        source_width:(source>>32) as u32,source_height:source as u32,
    }
}
static CURRENT:OnceLock<Mutex<Option<Session>>>=OnceLock::new();
fn current()->&'static Mutex<Option<Session>> {CURRENT.get_or_init(||Mutex::new(None))}

fn work_loop(
    rx:Receiver<Command>,started:SyncSender<Result<(),String>>,
    owner:Option<Arc<HqMappingOwner>>,
    actual_mode:Arc<AtomicU32>,source_size:Arc<AtomicU64>,
) {
    let mut lens=match Lens::create() {
        Ok(lens)=>lens,
        Err(e)=>{let _=started.send(Err(e));return},
    };
    // Register only for this opt-in session, from the same thread that runs
    // its Windows message pump. Drop always unhooks on exit/error.
    let _wheel=match ScopedWheelHook::install() {
        Ok(hook)=>hook,
        Err(e)=>{let _=started.send(Err(e));return},
    };
    // Independent BGRA layered window for the reviewed CPU reference path.
    // If the desktop compositor refuses it, keep Magnification.dll working.
    let mut hq=owner.as_ref().and_then(|_|HqDecodedCpuLens::create().ok());
    let _=started.send(Ok(()));
    loop {
        match rx.recv_timeout(Duration::from_millis(16)) {
            Ok(Command::Update(update))=> {
                if owner.is_none() || hq.is_none() {
                    lens.display(update);
                } else {
                    lens.view=Some(update);
                }
            },
            Err(std::sync::mpsc::RecvTimeoutError::Disconnected)=>break,
            Err(std::sync::mpsc::RecvTimeoutError::Timeout)=>{},
        }
        unsafe {
            let mut msg=Msg::default();
            while PeekMessageW(&mut msg,null_mut(),0,0,1)>0 {
                TranslateMessage(&msg);
                DispatchMessageW(&msg);
            }
        }
        lens.apply_wheel(hq.as_ref().is_none_or(|preview|!preview.is_presenting()));
        let mut presented_source=None;
        if let (Some(owner),Some(preview),Some(view))=(owner.as_ref(),hq.as_mut(),lens.view) {
            if !view.active || !view.inside() {
                preview.hide();
                lens.set_visible(false);
            } else if let Some(target)=lens_geometry(view,lens.settings) {
                let source=ScreenViewport {
                    x:view.x,y:view.y,width:view.width as u32,height:view.height as u32,
                };
                let _=preview.update_from_source(owner,source,
                    (view.cursor_x,view.cursor_y),target.size as u32,lens.settings.zoom);
                if preview.is_presenting() {
                    presented_source=preview.presented_source_size();
                    lens.set_visible(false);
                    // The wheel must still resize the HQ circle even though
                    // the Magnification.dll fallback HWND is hidden.
                    set_wheel_target(view);
                } else {
                    lens.display(view); // No valid source frame: never go black.
                }
            } else {
                preview.hide();
                lens.set_visible(false);
            }
        }
        // Report real HWND presentation state without copying or logging
        // any device-frame bytes, screen coordinates, tokens or thumbnails.
        if let Some((w,h))=presented_source {
            source_size.store(((w as u64)<<32) | h as u64,Ordering::Release);
            actual_mode.store(2,Ordering::Release);
        } else {
            source_size.store(0,Ordering::Release);
            let displaying_legacy=lens.view.is_some_and(|v|v.active && v.inside())
                && lens.visible;
            actual_mode.store(u32::from(displaying_legacy),Ordering::Release);
        }
        lens.refresh();
    }
    actual_mode.store(0,Ordering::Release);
    source_size.store(0,Ordering::Release);
    WHEEL_ACTIVE.store(0,Ordering::SeqCst);
}

pub fn start(device:&str)->Result<(),String> { start_with_hq(device,None) }

pub fn start_with_hq(device:&str,owner:Option<Arc<HqMappingOwner>>)->Result<(),String> {
    stop_all();
    let (tx,rx)=sync_channel(5);
    let (ready_tx,ready_rx)=sync_channel(1);
    let hq_requested=owner.is_some();
    let mode=Arc::new(AtomicU32::new(0));
    let source_size=Arc::new(AtomicU64::new(0));
    let mode_worker=mode.clone();
    let size_worker=source_size.clone();
    let worker=thread::Builder::new().name("mirroleap-live-magnifier".into())
        .spawn(move||work_loop(rx,ready_tx,owner,mode_worker,size_worker))
        .map_err(|e|format!("Cannot start native live magnifier: {e}"))?;
    match ready_rx.recv_timeout(Duration::from_secs(4)) {
        Ok(Ok(()))=>{
            let mut lock=current().lock().map_err(|e|e.to_string())?;
            *lock=Some(Session{device:device.to_string(),tx,thread:worker,
                mode,source_size,hq_requested});
            Ok(())
        },
        Ok(Err(e))=>{drop(tx);let _=worker.join();Err(e)},
        Err(_)=>{drop(tx);let _=worker.join();Err("Windows magnifier did not become ready".into())},
    }
}
pub fn stop(device:&str) {
    let old=current().lock().ok().and_then(|mut guard|{
        if guard.as_ref().is_some_and(|s|s.device==device) {guard.take()} else {None}
    });
    if let Some(session)=old {
        drop(session.tx);
        let _=session.thread.join();
    }
}
pub fn stop_all() {
    let old=current().lock().ok().and_then(|mut guard|guard.take());
    if let Some(session)=old {
        drop(session.tx);
        let _=session.thread.join();
    }
}
pub fn update(device:&str,view:ViewUpdate) {
    if let Ok(guard)=current().try_lock() {
        if let Some(session)=guard.as_ref().filter(|s|s.device==device) {
            match session.tx.try_send(Command::Update(view)) {
                Ok(()) | Err(TrySendError::Full(_))=>{},
                Err(TrySendError::Disconnected(_))=>{},
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn truthfully_defaults_off_without_an_active_hq_render() {
        let status=source_status("nonexistent-phone");
        assert_eq!(status.mode,"off");
        assert_eq!(status.source_width,0);
        assert!(!status.hq_requested);
    }
    #[test]
    fn native_circle_stays_inside_all_phone_edges_while_sample_tracks_pointer() {
        for (width,height) in [(420,900),(900,420),(220,400)] {
            for (dx,dy) in [(0,0),(width-1,0),(0,height-1),
                            (width-1,height-1),(width/2,height/2)] {
                let view=ViewUpdate{
                    x:100,y:-80,width,height,
                    cursor_x:100+dx,cursor_y:-80+dy,
                    active:true,frame_hwnd:0,controls_hwnd:0,
                };
                let rect=lens_geometry(view,LensSettings::default()).unwrap();
                assert!(rect.x>=view.x+4);
                assert!(rect.y>=view.y+4);
                assert!(rect.x+rect.size<=view.x+width-4);
                assert!(rect.y+rect.size<=view.y+height-4);
                let cx=(rect.source.left+rect.source.right)/2;
                let cy=(rect.source.top+rect.source.bottom)/2;
                assert!(cx.abs_diff(view.cursor_x)<=1);
                assert!(cy.abs_diff(view.cursor_y)<=1);
            }
        }
    }
    #[test]
    fn live_wheel_is_consumed_only_inside_foreground_phone_geometry() {
        assert!(inside_active_phone_rect(120,160,100,100,360,700));
        assert!(!inside_active_phone_rect(99,160,100,100,360,700));
        assert!(!inside_active_phone_rect(460,160,100,100,360,700));
        assert!(!inside_active_phone_rect(120,800,100,100,360,700));
        assert!(!inside_active_phone_rect(120,160,100,100,0,700));
    }
    #[test]
    fn creator_prefers_real_235_zoom_instead_of_hiding_blur_by_reducing_zoom() {
        let preset=LensSettings::default();
        assert!((preset.zoom-2.35).abs()<0.00001);
        assert!(preset.zoom>2.0);
        assert!((preset.diameter_ratio-0.50).abs()<0.00001);
    }

    #[test]
    fn wheel_changes_diameter_separately_from_optical_zoom() {
        let view=ViewUpdate{x:10,y:20,width:430,height:920,cursor_x:200,cursor_y:400,
            active:true,frame_hwnd:0,controls_hwnd:0};
        let mut settings=LensSettings::default();
        let initial=lens_geometry(view,settings).unwrap();
        let old_zoom=settings.zoom;
        assert!(settings.adjust_wheel(120,false));
        let larger=lens_geometry(view,settings).unwrap();
        assert!(larger.size>initial.size);
        assert_eq!(settings.zoom,old_zoom);
        assert!(settings.adjust_wheel(-120,false));
        assert!((settings.diameter_ratio-DEFAULT_DIAMETER_RATIO).abs()<0.001);

        let old_size=lens_geometry(view,settings).unwrap().size;
        assert!(settings.adjust_wheel(120,true));
        assert!(settings.zoom>old_zoom);
        let more_zoom=lens_geometry(view,settings).unwrap();
        assert_eq!(more_zoom.size,old_size);
        assert!(more_zoom.source.right-more_zoom.source.left <
                initial.source.right-initial.source.left);
    }

    #[test]
    fn wheel_settings_are_bounded_and_never_crop_the_lens() {
        let mut settings=LensSettings::default();
        for _ in 0..100 {
            settings.adjust_wheel(120,false);
            settings.adjust_wheel(120,true);
        }
        assert!((settings.diameter_ratio-DIAMETER_MAX).abs()<0.001);
        assert!((settings.zoom-FACTOR_MAX).abs()<0.001);
        let view=ViewUpdate{x:-100,y:30,width:320,height:660,
            cursor_x:219,cursor_y:689,active:true,frame_hwnd:0,controls_hwnd:0};
        let g=lens_geometry(view,settings).unwrap();
        assert!(g.x>=view.x+4 && g.x+g.size<=view.x+view.width-4);
        assert!(g.y>=view.y+4 && g.y+g.size<=view.y+view.height-4);
        for _ in 0..100 {
            settings.adjust_wheel(-120,false);
            settings.adjust_wheel(-120,true);
        }
        assert!((settings.diameter_ratio-DIAMETER_MIN).abs()<0.001);
        assert!((settings.zoom-FACTOR_MIN).abs()<0.001);
    }

    #[test]
    fn native_lens_pauses_outside_phone_or_in_background() {
        let view=ViewUpdate{x:10,y:10,width:430,height:920,cursor_x:30,cursor_y:30,
            active:true,frame_hwnd:0,controls_hwnd:0};
        assert!(lens_geometry(view,LensSettings::default()).is_some());
        assert!(lens_geometry(ViewUpdate{active:false,..view},LensSettings::default()).is_none());
        assert!(lens_geometry(ViewUpdate{cursor_x:0,..view},LensSettings::default()).is_none());
    }
}
