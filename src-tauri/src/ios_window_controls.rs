//! Two real native Windows controls for the AirPlay mirror. These are
//! same-process pop-up BUTTON controls, NOT children of foreign SDL windows
//! and NOT WebView2 surfaces. The receiver's title bar stays chromeless.
//! All HWND manipulation and cleanup happen on the iOS bezel worker thread.
#![cfg(target_os="windows")]

use std::{ffi::c_void,ptr,ptr::NonNull,sync::atomic::{AtomicIsize,AtomicU8,Ordering}};
use windows::Win32::Foundation::HWND;
use windows::Win32::Graphics::Gdi::{GetMonitorInfoW,MonitorFromWindow,MONITORINFO,MONITOR_DEFAULTTONEAREST};
const EX_TOOLWINDOW:u32=0x80;
const EX_NOACTIVATE:u32=0x0800_0000;
const STYLE_POPUP:u32=0x8000_0000;
const SW_HIDE:i32=0;
const SW_SHOW_NOACTIVATE:i32=4;
const SW_MINIMIZE:i32=6;
const GWLP_WNDPROC:i32=-4;
const WM_LBUTTONUP:u32=0x0202;
const SIZE:i32=32;
const SPACE:i32=4;
static MIN_HWND:AtomicIsize=AtomicIsize::new(0);
static CLOSE_HWND:AtomicIsize=AtomicIsize::new(0);
static OLD_MIN_PROC:AtomicIsize=AtomicIsize::new(0);
static OLD_CLOSE_PROC:AtomicIsize=AtomicIsize::new(0);
static ACTION:AtomicU8=AtomicU8::new(0);

#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub enum ControlAction{Minimize,Close}

#[link(name="user32")]
unsafe extern "system" {
    fn CreateWindowExW(ex:u32,class:*const u16,title:*const u16,style:u32,
        x:i32,y:i32,w:i32,h:i32,parent:*mut c_void,menu:*mut c_void,
        instance:*mut c_void,extra:*mut c_void)->*mut c_void;
    fn DestroyWindow(hwnd:*mut c_void)->i32;
    fn ShowWindow(hwnd:*mut c_void,command:i32)->i32;
    fn SetWindowLongPtrW(hwnd:*mut c_void,index:i32,new:isize)->isize;
    fn CallWindowProcW(old:isize,hwnd:*mut c_void,message:u32,wparam:usize,lparam:isize)->isize;
    fn DefWindowProcW(hwnd:*mut c_void,message:u32,wparam:usize,lparam:isize)->isize;
}

unsafe extern "system" fn button_proc(
    hwnd:*mut c_void,message:u32,wparam:usize,lparam:isize
)->isize{
    let is_min=hwnd as isize==MIN_HWND.load(Ordering::Relaxed);
    let is_close=hwnd as isize==CLOSE_HWND.load(Ordering::Relaxed);
    if message==WM_LBUTTONUP {
        if is_min{ACTION.store(1,Ordering::Release);}
        if is_close{ACTION.store(2,Ordering::Release);}
    }
    let old=if is_min{OLD_MIN_PROC.load(Ordering::Relaxed)}
        else if is_close{OLD_CLOSE_PROC.load(Ordering::Relaxed)}else{0};
    if old==0{unsafe{DefWindowProcW(hwnd,message,wparam,lparam)}}
    else{unsafe{CallWindowProcW(old,hwnd,message,wparam,lparam)}}
}

fn make_button(text:&str)->Result<NonNull<c_void>,&'static str>{
    let class:Vec<u16>="BUTTON".encode_utf16().chain(Some(0)).collect();
    let caption:Vec<u16>=text.encode_utf16().chain(Some(0)).collect();
    NonNull::new(unsafe{
        CreateWindowExW(EX_TOOLWINDOW|EX_NOACTIVATE,class.as_ptr(),caption.as_ptr(),
            STYLE_POPUP,0,0,SIZE,SIZE,
            ptr::null_mut(),ptr::null_mut(),ptr::null_mut(),ptr::null_mut())
    }).ok_or("Cannot create native AirPlay minimize/close button")
}

pub struct NativeControls{
    minimize:NonNull<c_void>,
    close:NonNull<c_void>,
    shown:bool,
}
impl NativeControls{
    pub fn create()->Result<Self,&'static str>{
        ACTION.store(0,Ordering::Release);
        let minimize=make_button("−")?;
        let close=match make_button("×"){
            Ok(hwnd)=>hwnd,
            Err(e)=>{unsafe{DestroyWindow(minimize.as_ptr());}return Err(e);}
        };
        MIN_HWND.store(minimize.as_ptr() as isize,Ordering::Release);
        CLOSE_HWND.store(close.as_ptr() as isize,Ordering::Release);
        let old_min=unsafe{SetWindowLongPtrW(
            minimize.as_ptr(),GWLP_WNDPROC,button_proc as usize as isize)};
        let old_close=unsafe{SetWindowLongPtrW(
            close.as_ptr(),GWLP_WNDPROC,button_proc as usize as isize)};
        OLD_MIN_PROC.store(old_min,Ordering::Release);
        OLD_CLOSE_PROC.store(old_close,Ordering::Release);
        if old_min==0 || old_close==0{
            // Ensure existing subclasses are not left with a dangling
            // callback when Windows refuses the other window's subclass.
            if old_min!=0{unsafe{SetWindowLongPtrW(minimize.as_ptr(),GWLP_WNDPROC,old_min);}}
            if old_close!=0{unsafe{SetWindowLongPtrW(close.as_ptr(),GWLP_WNDPROC,old_close);}}
            unsafe{DestroyWindow(close.as_ptr());DestroyWindow(minimize.as_ptr());}
            MIN_HWND.store(0,Ordering::Release);
            CLOSE_HWND.store(0,Ordering::Release);
            OLD_MIN_PROC.store(0,Ordering::Release);
            OLD_CLOSE_PROC.store(0,Ordering::Release);
            return Err("Cannot install native AirPlay control window handler");
        }
        Ok(Self{minimize,close,shown:false})
    }
    pub fn action()->Option<ControlAction>{
        match ACTION.swap(0,Ordering::AcqRel) {
            1=>Some(ControlAction::Minimize),
            2=>Some(ControlAction::Close),
            _=>None,
        }
    }
    pub fn hide(&mut self){
        if self.shown{
            unsafe{
                ShowWindow(self.minimize.as_ptr(),SW_HIDE);
                ShowWindow(self.close.as_ptr(),SW_HIDE);
            }
            self.shown=false;
        }
    }
    pub fn position(&mut self,video:HWND,x:i32,y:i32,width:i32)->Result<(),String>{
        let (min_x,min_y)=control_position(video,x,y,width);
        crate::device_frame::place_ios_companion(
            HWND(self.minimize.as_ptr()),video,min_x,min_y,SIZE,SIZE)?;
        crate::device_frame::place_ios_companion(
            HWND(self.close.as_ptr()),video,min_x+SIZE+SPACE,min_y,SIZE,SIZE)?;
        if !self.shown{
            unsafe{
                ShowWindow(self.minimize.as_ptr(),SW_SHOW_NOACTIVATE);
                ShowWindow(self.close.as_ptr(),SW_SHOW_NOACTIVATE);
            }
            self.shown=true;
        }
        Ok(())
    }
    pub fn minimize_receiver(video:HWND){
        unsafe{ShowWindow(video.0,SW_MINIMIZE);}
    }
}
fn control_position(video:HWND,x:i32,y:i32,width:i32)->(i32,i32){
    let x=x.saturating_add(width).saturating_sub(2*SIZE+SPACE+8);
    let mut top=y.saturating_sub(SIZE+5);
    let monitor=unsafe{MonitorFromWindow(video,MONITOR_DEFAULTTONEAREST)};
    if !monitor.0.is_null(){
        let mut info=MONITORINFO{cbSize:std::mem::size_of::<MONITORINFO>() as u32,
            ..Default::default()};
        if unsafe{GetMonitorInfoW(monitor,&mut info)}.as_bool() {
            top=top.max(info.rcMonitor.top+2);
        }
    }
    (x,top)
}
impl Drop for NativeControls{
    fn drop(&mut self){
        self.hide();
        let old_min=OLD_MIN_PROC.load(Ordering::Acquire);
        let old_close=OLD_CLOSE_PROC.load(Ordering::Acquire);
        if old_min!=0{
            unsafe{SetWindowLongPtrW(self.minimize.as_ptr(),GWLP_WNDPROC,old_min);}
        }
        if old_close!=0{
            unsafe{SetWindowLongPtrW(self.close.as_ptr(),GWLP_WNDPROC,old_close);}
        }
        unsafe{
            DestroyWindow(self.minimize.as_ptr());
            DestroyWindow(self.close.as_ptr());
        }
        MIN_HWND.store(0,Ordering::Release);
        CLOSE_HWND.store(0,Ordering::Release);
        OLD_MIN_PROC.store(0,Ordering::Release);
        OLD_CLOSE_PROC.store(0,Ordering::Release);
        ACTION.store(0,Ordering::Release);
    }
}

#[cfg(test)]
mod tests{
    use super::*;
    #[test]
    fn controls_have_distinct_commands(){
        assert_eq!(ControlAction::Minimize,ControlAction::Minimize);
        assert_ne!(ControlAction::Minimize,ControlAction::Close);
        assert_eq!(SIZE,32);
        assert_eq!(SPACE,4);
    }
}
