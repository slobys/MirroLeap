//! Two real native Windows controls for the AirPlay mirror. These are
//! same-process pop-up BUTTON controls, NOT children of foreign SDL windows
//! and NOT WebView2 surfaces. The receiver's title bar stays chromeless.
//! All HWND manipulation and cleanup happen on the iOS bezel worker thread.
#![cfg(target_os="windows")]

use std::{ffi::c_void,ptr,ptr::NonNull,sync::atomic::{AtomicIsize,AtomicU8,Ordering},time::{Duration,Instant}};
use windows::Win32::Foundation::{HWND,POINT};
use windows::Win32::Graphics::Gdi::{GetMonitorInfoW,MonitorFromWindow,MONITORINFO,MONITOR_DEFAULTTONEAREST,
    CreateRoundRectRgn,DeleteObject as GdiDeleteObject,HGDIOBJ,SetWindowRgn};
const EX_TOOLWINDOW:u32=0x80;
const EX_NOACTIVATE:u32=0x0800_0000;
const STYLE_POPUP:u32=0x8000_0000;
const SW_HIDE:i32=0;
const SW_SHOW_NOACTIVATE:i32=4;
const SW_MINIMIZE:i32=6;
const GWLP_WNDPROC:i32=-4;
const WM_PAINT:u32=0x000F;
const WM_ERASEBKGND:u32=0x0014;
const WM_LBUTTONUP:u32=0x0202;
const SIZE:i32=46;
const SPACE:i32=6;
const HIDE_AFTER_MS:u64=1400;
const START_HINT_MS:u64=650;
static MIN_HWND:AtomicIsize=AtomicIsize::new(0);
static CLOSE_HWND:AtomicIsize=AtomicIsize::new(0);
static OLD_MIN_PROC:AtomicIsize=AtomicIsize::new(0);
static OLD_CLOSE_PROC:AtomicIsize=AtomicIsize::new(0);
static ACTION:AtomicU8=AtomicU8::new(0);

#[repr(C)]
struct PaintStruct{
    hdc:*mut c_void,
    erase:i32,
    paint:windows::Win32::Foundation::RECT,
    restore:i32,
    inc_update:i32,
    reserved:[u8;32],
}
impl Default for PaintStruct {
    fn default()->Self{unsafe{std::mem::zeroed()}}
}
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
struct ButtonColors{fill:u32,outline:u32,icon:u32}
/// Graphite iPad Air control (COLORREF is 0x00BBGGRR).
fn button_colors(close:bool)->ButtonColors{
    ButtonColors{
        fill:0x00231B17,
        outline:0x006F635A,
        icon:if close{0x00CCDDEB}else{0x00F2E8DB},
    }
}

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
    fn GetCursorPos(cursor:*mut POINT)->i32;
    fn BeginPaint(hwnd:*mut c_void,paint:*mut PaintStruct)->*mut c_void;
    fn EndPaint(hwnd:*mut c_void,paint:*const PaintStruct)->i32;
}
#[link(name="gdi32")]
unsafe extern "system" {
    fn CreateSolidBrush(color:u32)->*mut c_void;
    fn CreatePen(style:i32,width:i32,color:u32)->*mut c_void;
    fn SelectObject(hdc:*mut c_void,obj:*mut c_void)->*mut c_void;
    fn DeleteObject(obj:*mut c_void)->i32;
    fn RoundRect(hdc:*mut c_void,left:i32,top:i32,right:i32,bottom:i32,
        arc_width:i32,arc_height:i32)->i32;
    fn MoveToEx(hdc:*mut c_void,x:i32,y:i32,old:*mut POINT)->i32;
    fn LineTo(hdc:*mut c_void,x:i32,y:i32)->i32;
}
fn paint_button(hwnd:*mut c_void,close:bool){
    let mut ps=PaintStruct::default();
    let dc=unsafe{BeginPaint(hwnd,&mut ps)};
    if dc.is_null(){return;}
    let colors=button_colors(close);
    let fill=unsafe{CreateSolidBrush(colors.fill)};
    let outline=unsafe{CreatePen(0,1,colors.outline)};
    if !fill.is_null() && !outline.is_null() {
        let old_brush=unsafe{SelectObject(dc,fill)};
        let old_pen=unsafe{SelectObject(dc,outline)};
        unsafe{RoundRect(dc,1,1,SIZE-1,SIZE-1,18,18);}
        if !old_brush.is_null(){unsafe{SelectObject(dc,old_brush);}}
        if !old_pen.is_null(){unsafe{SelectObject(dc,old_pen);}}
    }
    if !outline.is_null(){unsafe{DeleteObject(outline);}}
    if !fill.is_null(){unsafe{DeleteObject(fill);}}
    let icon_pen=unsafe{CreatePen(0,2,colors.icon)};
    if !icon_pen.is_null(){
        let old=unsafe{SelectObject(dc,icon_pen)};
        let center=SIZE/2;
        unsafe{
            if close {
                MoveToEx(dc,center-7,center-7,ptr::null_mut());
                LineTo(dc,center+7,center+7);
                MoveToEx(dc,center+7,center-7,ptr::null_mut());
                LineTo(dc,center-7,center+7);
            }else{
                MoveToEx(dc,center-8,center+3,ptr::null_mut());
                LineTo(dc,center+8,center+3);
            }
            if !old.is_null(){SelectObject(dc,old);}
            DeleteObject(icon_pen);
        }
    }
    unsafe{EndPaint(hwnd,&ps);}
}

unsafe extern "system" fn button_proc(
    hwnd:*mut c_void,message:u32,wparam:usize,lparam:isize
)->isize{
    let is_min=hwnd as isize==MIN_HWND.load(Ordering::Relaxed);
    let is_close=hwnd as isize==CLOSE_HWND.load(Ordering::Relaxed);
    if message==WM_ERASEBKGND{return 1;} // No white default BUTTON flash.
    if message==WM_PAINT {
        paint_button(hwnd,is_close);
        return 0;
    }
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
    let hwnd=NonNull::new(unsafe{
        CreateWindowExW(EX_TOOLWINDOW|EX_NOACTIVATE,class.as_ptr(),caption.as_ptr(),
            STYLE_POPUP,0,0,SIZE,SIZE,
            ptr::null_mut(),ptr::null_mut(),ptr::null_mut(),ptr::null_mut())
    }).ok_or("Cannot create native AirPlay minimize/close button")?;
    // Large soft-corner hit targets, instead of square 32px default boxes.
    let region=unsafe{CreateRoundRectRgn(0,0,SIZE+1,SIZE+1,16,16)};
    if !region.0.is_null() {
        if unsafe{SetWindowRgn(HWND(hwnd.as_ptr()),Some(region),true)}==0{
            let _=unsafe{GdiDeleteObject(HGDIOBJ(region.0))};
        }
    }
    Ok(hwnd)
}

pub struct NativeControls{
    minimize:NonNull<c_void>,
    close:NonNull<c_void>,
    shown:bool,
    created_at:Instant,
    last_nearby:Instant,
    last_placement:Option<(usize,i32,i32)>,
    stacked_at:Instant,
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
        Ok(Self{minimize,close,shown:false,
            created_at:Instant::now(),last_nearby:Instant::now(),
            last_placement:None,stacked_at:Instant::now()-Duration::from_secs(2)})
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
        self.last_placement=None;
    }
    /// Follow the receiver only while the pointer is near its top-right
    /// controls. Never leave distracting buttons over the iPad when idle.
    /// Include the space directly above the video where controls appear,
    /// so a hidden button can be discovered with a simple mouse movement.
    pub fn update_hover(&mut self,video:HWND,x:i32,y:i32,width:i32){
        let mut cursor=POINT::default();
        let nearby=unsafe{GetCursorPos(&mut cursor)}!=0 &&
            near_controls(cursor.x,cursor.y,x,y,width);
        if nearby{self.last_nearby=Instant::now();}
        let active=nearby ||
            self.last_nearby.elapsed()<Duration::from_millis(HIDE_AFTER_MS) ||
            self.created_at.elapsed()<Duration::from_millis(START_HINT_MS);
        if active{
            if self.position(video,x,y,width).is_err(){self.hide();}
        }else{
            self.hide();
        }
    }
    pub fn position(&mut self,video:HWND,x:i32,y:i32,width:i32)->Result<(),String>{
        let (min_x,min_y)=control_position(video,x,y,width);
        let next=(video.0 as usize,min_x,min_y);
        // Re-stacking two foreign z-order companions every 16ms made the
        // default buttons flash repeatedly as they crossed SDL's layer.
        // Move only when geometry changes, on first reveal, or for a slow
        // 2-second resilience refresh; stationary hover must not repaint.
        if stable_placement_needed(self.last_placement,Some(next),
            self.shown,self.stacked_at.elapsed()){
            crate::device_frame::place_ios_companion(
                HWND(self.minimize.as_ptr()),video,min_x,min_y,SIZE,SIZE)?;
            crate::device_frame::place_ios_companion(
                HWND(self.close.as_ptr()),video,min_x+SIZE+SPACE,min_y,SIZE,SIZE)?;
            self.last_placement=Some(next);
            self.stacked_at=Instant::now();
        }
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
fn stable_placement_needed(
    previous:Option<(usize,i32,i32)>,
    requested:Option<(usize,i32,i32)>,
    visible:bool,
    since_last:Duration,
)->bool{
    !visible || previous!=requested || since_last>=Duration::from_secs(2)
}
fn near_controls(mx:i32,my:i32,x:i32,y:i32,width:i32)->bool{
    mx>=x.saturating_add(width).saturating_sub(185)
        && mx<=x.saturating_add(width).saturating_add(16)
        && my>=y.saturating_sub(SIZE+26)
        && my<=y.saturating_add(100)
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
        assert_eq!(SIZE,46);
        assert_eq!(SPACE,6);
    }
    #[test]
    fn hover_displays_native_buttons_only_near_receiver_top_right(){
        assert!(near_controls(920,65,0,75,970));
        assert!(near_controls(970,20,0,75,970));
        assert!(!near_controls(500,65,0,75,970));
        assert!(!near_controls(920,600,0,75,970));
    }
    #[test]
    fn steady_hover_does_not_restack_or_flicker_every_frame(){
        let pos=Some((123usize,800,36));
        assert!(stable_placement_needed(None,pos,false,Duration::ZERO));
        assert!(!stable_placement_needed(pos,pos,true,Duration::from_millis(16)));
        assert!(!stable_placement_needed(pos,pos,true,Duration::from_millis(950)));
        assert!(stable_placement_needed(pos,pos,true,Duration::from_secs(2)));
        assert!(stable_placement_needed(pos,Some((123,801,36)),true,
            Duration::from_millis(16)));
    }
    #[test]
    fn graphite_control_palette_never_uses_a_white_button_background(){
        let minimize=button_colors(false);
        let close=button_colors(true);
        assert_eq!(minimize.fill,close.fill);
        assert_ne!(minimize.fill,0x00FF_FFFF);
        assert_eq!(minimize.outline,close.outline);
        assert_ne!(minimize.icon,close.icon);
    }
}
