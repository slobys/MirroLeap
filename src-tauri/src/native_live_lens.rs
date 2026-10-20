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
    sync::{mpsc::{sync_channel, Receiver, SyncSender, TrySendError}, Mutex, OnceLock},
    thread::{self, JoinHandle},
    time::Duration,
};

type RawHwnd = *mut c_void;
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
const PREVIEW_FACTOR: f32 = 2.35;
const PREVIEW_MIN: i32 = 140;
const PREVIEW_MAX: i32 = 450;

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
struct Geometry { x:i32, y:i32, size:i32, source:Rect }
fn lens_geometry(view:ViewUpdate)->Option<Geometry> {
    if !view.active || !view.inside() { return None; }
    let size=((view.width.min(view.height) as f32)*0.50).round() as i32;
    let size=size.clamp(PREVIEW_MIN,PREVIEW_MAX)
        .min(view.width.saturating_sub(8))
        .min(view.height.saturating_sub(8));
    if size<60 {return None;}
    let x=(view.cursor_x-size/2).clamp(view.x+4,view.x+view.width-size-4);
    let y=(view.cursor_y-size/2).clamp(view.y+4,view.y+view.height-size-4);
    // Keep the source CENTERED on the actual Android pointer even when the
    // visible lens is shifted inward to avoid clipping edge applications.
    let src=(size as f32/PREVIEW_FACTOR).round() as i32;
    let src=src.max(2);
    Some(Geometry {
        x,y,size,
        source:Rect {
            left:view.cursor_x-src/2, top:view.cursor_y-src/2,
            right:view.cursor_x-src/2+src, bottom:view.cursor_y-src/2+src,
        },
    })
}

struct Lens { host:RawHwnd, magnifier:RawHwnd, visible:bool, size:i32, exclusion:(usize,usize) }
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
            let transform=MagTransform {v:[
                [PREVIEW_FACTOR,0.,0.],
                [0.,PREVIEW_FACTOR,0.],
                [0.,0.,1.],
            ]};
            if MagSetWindowTransform(control,&transform)==0 {
                DestroyWindow(control);
                DestroyWindow(host);
                MagUninitialize();
                return Err("Windows could not enable live graphics magnification".into());
            }
            Ok(Self{host,magnifier:control,visible:false,size:0,exclusion:(0,0)})
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
        if self.visible==value {return;}
        if !value {unsafe {ShowWindow(self.host,0);}}
        self.visible=value;
    }
    fn display(&mut self,view:ViewUpdate) {
        let Some(target)=lens_geometry(view) else {self.set_visible(false);return;};
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
        }
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
struct Session { device:String, tx:SyncSender<Command>, thread:JoinHandle<()> }
static CURRENT:OnceLock<Mutex<Option<Session>>>=OnceLock::new();
fn current()->&'static Mutex<Option<Session>> {CURRENT.get_or_init(||Mutex::new(None))}

fn work_loop(rx:Receiver<Command>,started:SyncSender<Result<(),String>>) {
    let mut lens=match Lens::create() {
        Ok(lens)=>{let _=started.send(Ok(()));lens},
        Err(e)=>{let _=started.send(Err(e));return},
    };
    loop {
        match rx.recv_timeout(Duration::from_millis(16)) {
            Ok(Command::Update(update))=>lens.display(update),
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
        lens.refresh();
    }
}

pub fn start(device:&str)->Result<(),String> {
    stop_all();
    let (tx,rx)=sync_channel(5);
    let (ready_tx,ready_rx)=sync_channel(1);
    let worker=thread::Builder::new().name("mirroleap-live-magnifier".into())
        .spawn(move||work_loop(rx,ready_tx))
        .map_err(|e|format!("Cannot start native live magnifier: {e}"))?;
    match ready_rx.recv_timeout(Duration::from_secs(4)) {
        Ok(Ok(()))=>{
            let mut lock=current().lock().map_err(|e|e.to_string())?;
            *lock=Some(Session{device:device.to_string(),tx,thread:worker});
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
    fn native_circle_stays_inside_all_phone_edges_while_sample_tracks_pointer() {
        for (width,height) in [(420,900),(900,420),(220,400)] {
            for (dx,dy) in [(0,0),(width-1,0),(0,height-1),
                            (width-1,height-1),(width/2,height/2)] {
                let view=ViewUpdate{
                    x:100,y:-80,width,height,
                    cursor_x:100+dx,cursor_y:-80+dy,
                    active:true,frame_hwnd:0,controls_hwnd:0,
                };
                let rect=lens_geometry(view).unwrap();
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
    fn native_lens_pauses_outside_phone_or_in_background() {
        let view=ViewUpdate{x:10,y:10,width:430,height:920,cursor_x:30,cursor_y:30,
            active:true,frame_hwnd:0,controls_hwnd:0};
        assert!(lens_geometry(view).is_some());
        assert!(lens_geometry(ViewUpdate{active:false,..view}).is_none());
        assert!(lens_geometry(ViewUpdate{cursor_x:0,..view}).is_none());
    }
}
