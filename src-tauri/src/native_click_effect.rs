//! Mouse-through native Win32 click ripples for MirroCreator.
//! WebView2 overlays can be hidden behind SDL video when focus/topmost changes.
//! This independent HWND is layered above the real phone video; it does not
//! synthesize clicks, collect coordinates in logs, capture phone pixels or
//! require Android permissions.
#![cfg(target_os = "windows")]

use std::{
    collections::HashMap, ffi::c_void, ptr::{null, null_mut},
    sync::{mpsc::{sync_channel, Receiver, SyncSender, TrySendError}, Mutex, OnceLock},
    thread::{self, JoinHandle}, time::{Duration, Instant},
};

type Handle=*mut c_void;
const EX_LAYERED:u32=0x0008_0000;
const EX_TRANSPARENT:u32=0x0000_0020;
const EX_TOOLWINDOW:u32=0x0000_0080;
const EX_NOACTIVATE:u32=0x0800_0000;
const EX_TOPMOST:u32=0x0000_0008;
const POPUP:u32=0x8000_0000;
const SWP_NOACTIVATE:u32=0x0010;
const SWP_SHOWWINDOW:u32=0x0040;
const ULW_ALPHA:u32=2;
const DIB_RGB_COLORS:u32=0;
const BI_RGB:u32=0;
const SIDE:i32=184;
const DURATION_MS:u64=760;
const AC_SRC_OVER:u8=0;
const AC_SRC_ALPHA:u8=1;

#[repr(C)]
#[derive(Clone,Copy,Default)]
struct Point {x:i32,y:i32}
#[repr(C)]
#[derive(Clone,Copy,Default)]
struct Size {cx:i32,cy:i32}
#[repr(C)]
#[derive(Clone,Copy)]
struct Blend {operation:u8,flags:u8,alpha:u8,format:u8}
#[repr(C)]
#[derive(Clone,Copy)]
struct BitmapInfoHeader {
    size:u32,width:i32,height:i32,planes:u16,bit_count:u16,
    compression:u32,size_image:u32,xpels:i32,ypels:i32,
    used:u32,important:u32,
}
#[repr(C)]
struct BitmapInfo {header:BitmapInfoHeader, colors:[u32;1]}
#[repr(C)]
#[derive(Clone,Copy)]
struct Msg {hwnd:Handle,message:u32,wparam:usize,lparam:isize,time:u32,
    pt:Point,private:u32}
impl Msg {fn blank()->Self {unsafe{std::mem::zeroed()}}}

#[link(name="user32")]
unsafe extern "system" {
    fn CreateWindowExW(ex:u32,class:*const u16,title:*const u16,style:u32,
        x:i32,y:i32,w:i32,h:i32,parent:Handle,menu:Handle,
        instance:Handle,extra:*mut c_void)->Handle;
    fn UpdateLayeredWindow(hwnd:Handle,target:Handle,dest:*const Point,
        size:*const Size,source:Handle,origin:*const Point,
        key:u32,blend:*const Blend,flags:u32)->i32;
    fn SetWindowPos(hwnd:Handle,after:Handle,x:i32,y:i32,w:i32,h:i32,flags:u32)->i32;
    fn ShowWindow(hwnd:Handle,how:i32)->i32;
    fn DestroyWindow(hwnd:Handle)->i32;
    fn PeekMessageW(msg:*mut Msg,hwnd:Handle,min:u32,max:u32,remove:u32)->i32;
    fn TranslateMessage(msg:*const Msg)->i32;
    fn DispatchMessageW(msg:*const Msg)->isize;
}
#[link(name="gdi32")]
unsafe extern "system" {
    fn CreateCompatibleDC(source:Handle)->Handle;
    fn CreateDIBSection(hdc:Handle,info:*const BitmapInfo,usage:u32,
        out:*mut *mut c_void,section:Handle,offset:u32)->Handle;
    fn SelectObject(hdc:Handle,object:Handle)->Handle;
    fn DeleteObject(object:Handle)->i32;
    fn DeleteDC(hdc:Handle)->i32;
}

fn wide(value:&str)->Vec<u16>{value.encode_utf16().chain(Some(0)).collect()}
fn blended(alpha:f32,r:u8,g:u8,b:u8)->[u8;4] {
    let a=(alpha.clamp(0.,1.)*255.).round() as u16;
    [(b as u16*a/255) as u8,(g as u16*a/255) as u8,
     (r as u16*a/255) as u8,a as u8]
}
fn ring_pixel(x:f32,y:f32,progress:f32)->[u8;4] {
    let px=x-SIDE as f32/2.;
    let py=y-SIDE as f32/2.;
    let distance=(px*px+py*py).sqrt();
    let progress=progress.clamp(0.,1.);
    let radius=17.+66.*progress;
    let thickness=4.2-progress*1.4;
    let main=(1.-((distance-radius)/thickness).abs()).clamp(0.,1.);
    let secondary=(1.-((distance-radius*.86)/(thickness*.9)).abs()).clamp(0.,1.);
    let core=if progress < .28 {
        (1.-progress/.28)*((6.-distance)/2.).clamp(0.,1.)
    } else {0.};
    let fade=(1.-progress).powf(.70);
    if core>=main && core>=secondary {
        blended(core*fade,255,255,255)
    } else if main>secondary {
        blended(main*fade*.98,10,242,191)
    } else {
        blended(secondary*fade*.85,255,255,255)
    }
}

struct Layer {
    window:Handle,dc:Handle,bitmap:Handle,old_bitmap:Handle,
    pixels:*mut u8,visible:bool,
}
impl Layer {
    fn new()->Result<Self,String>{
        unsafe {
            let class=wide("STATIC");
            let title=wide("MirroLeap click ripple");
            let window=CreateWindowExW(
                EX_LAYERED|EX_TRANSPARENT|EX_TOOLWINDOW|EX_NOACTIVATE|EX_TOPMOST,
                class.as_ptr(),title.as_ptr(),POPUP,0,0,SIDE,SIDE,
                null_mut(),null_mut(),null_mut(),null_mut());
            if window.is_null() {return Err("Could not create native click effect".into());}
            let dc=CreateCompatibleDC(null_mut());
            if dc.is_null(){DestroyWindow(window);return Err("Could not create click effect DC".into());}
            let info=BitmapInfo {
                header:BitmapInfoHeader {
                    size:std::mem::size_of::<BitmapInfoHeader>() as u32,
                    width:SIDE,height:-SIDE,planes:1,bit_count:32,compression:BI_RGB,
                    size_image:0,xpels:0,ypels:0,used:0,important:0,
                },colors:[0],
            };
            let mut bits:*mut c_void=null_mut();
            let bitmap=CreateDIBSection(dc,&info,DIB_RGB_COLORS,
                &mut bits,null_mut(),0);
            if bitmap.is_null() || bits.is_null() {
                DeleteDC(dc);DestroyWindow(window);
                return Err("Could not allocate click effect pixels".into());
            }
            let old_bitmap=SelectObject(dc,bitmap);
            Ok(Self{window,dc,bitmap,old_bitmap,pixels:bits as *mut u8,
                visible:false})
        }
    }
    fn draw(&mut self,center:Point,progress:f32)->bool {
        // Alpha-premultiplied BGRA: no color-key rectangle, black corners or
        // flickering GDI paint over the real phone.
        let buffer=unsafe {std::slice::from_raw_parts_mut(self.pixels,
            SIDE as usize*SIDE as usize*4)};
        for y in 0..SIDE {
            for x in 0..SIDE {
                let offset=((y*SIDE+x)*4) as usize;
                buffer[offset..offset+4]
                    .copy_from_slice(&ring_pixel(x as f32,y as f32,progress));
            }
        }
        let target=Point{x:center.x-SIDE/2,y:center.y-SIDE/2};
        let size=Size{cx:SIDE,cy:SIDE};
        let origin=Point::default();
        let blend=Blend{operation:AC_SRC_OVER,flags:0,alpha:255,format:AC_SRC_ALPHA};
        unsafe {
            if UpdateLayeredWindow(self.window,null_mut(),&target,&size,self.dc,
                &origin,0,&blend,ULW_ALPHA)==0 {return false;}
            if SetWindowPos(self.window,(-1_isize) as Handle,target.x,target.y,
                SIDE,SIDE,SWP_NOACTIVATE|SWP_SHOWWINDOW)==0 {return false;}
            self.visible=true;
        }
        true
    }
    fn hide(&mut self){
        if self.visible {unsafe{ShowWindow(self.window,0);} self.visible=false;}
    }
}
impl Drop for Layer {
    fn drop(&mut self) {
        self.hide();
        unsafe {
            SelectObject(self.dc,self.old_bitmap);
            DeleteObject(self.bitmap);
            DeleteDC(self.dc);
            DestroyWindow(self.window);
        }
    }
}
enum Command {Flash(Point,SyncSender<bool>)}
struct Session {tx:SyncSender<Command>,thread:JoinHandle<()>}
static SESSIONS:OnceLock<Mutex<HashMap<String,Session>>>=OnceLock::new();
fn sessions()->&'static Mutex<HashMap<String,Session>>{
    SESSIONS.get_or_init(||Mutex::new(HashMap::new()))
}
fn run(rx:Receiver<Command>,ready:SyncSender<Result<(),String>>){
    let mut layer=match Layer::new(){
        Ok(layer)=>layer,
        Err(error)=>{let _=ready.send(Err(error));return}
    };
    let _=ready.send(Ok(()));
    let mut last:Option<(Point,Instant)>=None;
    loop {
        match rx.recv_timeout(Duration::from_millis(16)){
            Ok(Command::Flash(point,ack))=>{
                // Signal only if Windows actually accepted the fully-alpha
                // native layered frame AND the topmost position.
                let visible=layer.draw(point,0.);
                let _=ack.send(visible);
                last=visible.then_some((point,Instant::now()));
            },
            Err(std::sync::mpsc::RecvTimeoutError::Disconnected)=>break,
            Err(std::sync::mpsc::RecvTimeoutError::Timeout)=>{},
        }
        unsafe {
            let mut msg=Msg::blank();
            while PeekMessageW(&mut msg,null_mut(),0,0,1)>0 {
                TranslateMessage(&msg);DispatchMessageW(&msg);
            }
        }
        if let Some((position,start))=last {
            let elapsed=start.elapsed();
            if elapsed.as_millis()>=DURATION_MS as u128 {
                last=None;layer.hide();
            } else {
                if !layer.draw(position,elapsed.as_secs_f32()/
                    Duration::from_millis(DURATION_MS).as_secs_f32()){
                    layer.hide();
                }
            }
        }
    }
}
pub(crate) fn start(device:&str)->Result<(),String>{
    stop(device);
    let (tx,rx)=sync_channel(16);
    let (ready_tx,ready_rx)=sync_channel(1);
    let thread=thread::Builder::new().name("mirroleap-native-click-ripple".into())
        .spawn(move||run(rx,ready_tx))
        .map_err(|e|format!("Cannot create click effect worker: {e}"))?;
    match ready_rx.recv_timeout(Duration::from_secs(3)) {
        Ok(Ok(()))=>{
            sessions().lock().map_err(|e|e.to_string())?
                .insert(device.to_string(),Session{tx,thread});
            Ok(())
        },
        Ok(Err(error))=>{drop(tx);let _=thread.join();Err(error)},
        Err(_)=>{drop(tx);let _=thread.join();Err("Native click effect timed out".into())}
    }
}
pub(crate) fn flash(device:&str,x:i32,y:i32)->bool{
    let (tx,rx)=sync_channel(1);
    let accepted=sessions().lock().ok().and_then(|entries|
        entries.get(device).map(|session|
            session.tx.try_send(Command::Flash(Point{x,y},tx)).is_ok()
        )).unwrap_or(false);
    accepted && rx.recv_timeout(Duration::from_millis(220)).unwrap_or(false)
}
pub(crate) fn stop(device:&str){
    let old=sessions().lock().ok().and_then(|mut sessions|sessions.remove(device));
    if let Some(old)=old {drop(old.tx);let _=old.thread.join();}
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn alpha_ring_is_transparent_outside_and_visible_during_a_click(){
        assert_eq!(ring_pixel(0.,0.,0.)[3],0);
        assert!(ring_pixel(SIDE as f32/2.+17.,SIDE as f32/2.,0.)[3]>200);
        assert!(ring_pixel(SIDE as f32/2.+42.,SIDE as f32/2.,.4)[3]>50);
        assert_eq!(ring_pixel(SIDE as f32/2.,SIDE as f32/2.,1.)[3],0);
    }
    #[test]
    fn stored_pixels_are_premultiplied_alpha(){
        let px=blended(.3,10,240,180);
        assert!(px[3]>0 && px[3]<255);
        assert!(px[0]<=px[3] && px[1]<=px[3] && px[2]<=px[3]);
    }
}
