//! Mouse-through native Win32 click ripples for MirroCreator.
//! WebView2 overlays can be hidden behind SDL video when focus/topmost changes.
//! This independent HWND is layered above the real phone video; it does not
//! synthesize clicks, collect coordinates in logs, capture phone pixels or
//! require Android permissions.
#![cfg(target_os = "windows")]

use std::{
    collections::HashMap, ffi::c_void, ptr::null_mut,
    sync::{mpsc::{sync_channel, Receiver, SyncSender}, Mutex, OnceLock},
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
// 1.8 s with a long fully-visible phase: the earlier 760 ms, thin ring
// often disappeared before the user could look back from the toolbar.
const SIDE:i32=240;
const DURATION_MS:u64=1800;
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
/// All per-frame calculations happen ONCE, not 57,600 times per frame.
/// Native layered-pixel output is premultiplied BGRA for UpdateLayeredWindow.
#[derive(Clone,Copy)]
struct RippleStyle {
    radius:f32, thickness:f32, fade:f32, dot_radius:f32,
}
impl RippleStyle {
    fn at(progress:f32)->Self {
        let p=progress.clamp(0.,1.);
        Self {
            radius:24.+76.*p,
            thickness:7.-2.*p,
            // A recognisable ring is visible for about 1.25 seconds,
            // then fades instead of vanishing after a fraction of a second.
            fade:if p<=0.70 {1.} else {((1.-p)/0.30).clamp(0.,1.)},
            dot_radius:8.5*(1.-p*0.55),
        }
    }
}
fn ring_pixel_prepared(x:f32,y:f32,style:RippleStyle)->[u8;4] {
    let px=x+0.5-SIDE as f32/2.;
    let py=y+0.5-SIDE as f32/2.;
    let distance=(px*px+py*py).sqrt();
    let diff=(distance-style.radius).abs();
    // An almost-opaque cyan ring with a charcoal outline reads on BOTH
    // light backgrounds and the dark Xiaomi/Android home-screen wallpaper.
    let cyan=(style.thickness+0.7-diff).clamp(0.,1.);
    let dark=(style.thickness+4.2-diff).clamp(0.,1.);
    let white=(3.1-(distance-style.radius*0.78).abs()).clamp(0.,1.);
    let dot=(style.dot_radius+0.8-distance).clamp(0.,1.);
    let fade=style.fade;
    if dot>0. {
        blended(dot*fade,255,246,135)
    } else if cyan>0. {
        blended(cyan*fade,10,255,214)
    } else if white>0. {
        blended(white*fade*0.91,255,255,255)
    } else if dark>0. {
        blended(dark*fade*0.92,8,23,41)
    } else {
        [0;4]
    }
}
#[cfg(test)]
fn ring_pixel(x:f32,y:f32,progress:f32)->[u8;4] {
    ring_pixel_prepared(x,y,RippleStyle::at(progress))
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
        let style=RippleStyle::at(progress);
        for y in 0..SIDE {
            for x in 0..SIDE {
                let offset=((y*SIDE+x)*4) as usize;
                buffer[offset..offset+4]
                    .copy_from_slice(&ring_pixel_prepared(x as f32,y as f32,style));
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
    // CAPTUREBLT includes WS_EX_LAYERED windows. The old tests examined a
    // BGRA buffer only, so UpdateLayeredWindow(true) could be reported as
    // "paint confirmed" even when the compositor showed no visible ring.
    #[cfg(target_os="windows")]
    #[link(name="user32")]
    unsafe extern "system" {
        fn GetDC(hwnd:Handle)->Handle;
        fn ReleaseDC(hwnd:Handle,dc:Handle)->i32;
        fn GetSystemMetrics(index:i32)->i32;
    }
    #[cfg(target_os="windows")]
    #[link(name="gdi32")]
    unsafe extern "system" {
        fn BitBlt(dst:Handle,dx:i32,dy:i32,w:i32,h:i32,
            src:Handle,sx:i32,sy:i32,rop:u32)->i32;
    }
    #[cfg(target_os="windows")]
    fn capture_composited_rect(left:i32,top:i32,side:i32)->Result<Vec<u8>,String> {
        unsafe {
            let screen=GetDC(null_mut());
            if screen.is_null() {return Err("No desktop DC".into());}
            let memory=CreateCompatibleDC(screen);
            if memory.is_null() {
                ReleaseDC(null_mut(),screen);
                return Err("No screenshot memory DC".into());
            }
            let info=BitmapInfo {
                header:BitmapInfoHeader {
                    size:std::mem::size_of::<BitmapInfoHeader>() as u32,
                    width:side,height:-side,planes:1,bit_count:32,
                    compression:BI_RGB,size_image:0,xpels:0,ypels:0,
                    used:0,important:0,
                }, colors:[0],
            };
            let mut pixels:*mut c_void=null_mut();
            let bitmap=CreateDIBSection(memory,&info,DIB_RGB_COLORS,
                &mut pixels,null_mut(),0);
            if bitmap.is_null() || pixels.is_null() {
                DeleteDC(memory);ReleaseDC(null_mut(),screen);
                return Err("No desktop screenshot bitmap".into());
            }
            let previous=SelectObject(memory,bitmap);
            let copied=BitBlt(memory,0,0,side,side,screen,left,top,
                0x00CC_0020 | 0x4000_0000); // SRCCOPY | CAPTUREBLT
            let bytes=if copied!=0 {
                std::slice::from_raw_parts(pixels as *const u8,
                    (side*side*4) as usize).to_vec()
            } else {Vec::new()};
            SelectObject(memory,previous);
            DeleteObject(bitmap);DeleteDC(memory);
            ReleaseDC(null_mut(),screen);
            if copied==0 {return Err("CAPTUREBLT screen read failed".into());}
            Ok(bytes)
        }
    }
    #[test]
    fn layered_click_is_visible_in_real_desktop_capture() {
        if std::env::var_os("MIRROLEAP_CLICK_VISUAL_SMOKE").is_none() {return;}
        // CI runs this on a real Windows desktop after WebView2 GUI smoke.
        // Capture no device/Android pixels, save nothing, report counts only.
        let x=unsafe{GetSystemMetrics(76)}; // SM_XVIRTUALSCREEN
        let y=unsafe{GetSystemMetrics(77)}; // SM_YVIRTUALSCREEN
        let w=unsafe{GetSystemMetrics(78)}; // SM_CXVIRTUALSCREEN
        let h=unsafe{GetSystemMetrics(79)}; // SM_CYVIRTUALSCREEN
        assert!(w>300 && h>300,"No interactive Windows desktop available");
        let center=Point{x:x+w/2,y:y+h/2};
        let side=156;
        let left=center.x-side/2;
        let top=center.y-side/2;
        let background=capture_composited_rect(left,top,side)
            .expect("Read baseline Windows desktop pixels");
        let mut layer=Layer::new().expect("Native HWND and BGRA surface");
        assert!(layer.draw(center,0.14),"Windows refused layered click frame");
        std::thread::sleep(Duration::from_millis(120));
        let actual=capture_composited_rect(left,top,side)
            .expect("Read composited Windows desktop including click overlay");
        layer.hide();
        let (mut differences,mut high_contrast)=(0usize,0usize);
        for (before,after) in background.chunks_exact(4).zip(actual.chunks_exact(4)) {
            let delta=(0..3).map(|n|before[n].abs_diff(after[n]) as u32)
                .sum::<u32>();
            if delta>95 {differences+=1;}
            // Teal/bright yellow on top of arbitrary desktop wallpaper.
            if delta>95 && after[1] as i32 > after[2] as i32+55 &&
               after[1] as i32 > after[0] as i32+20 {
                high_contrast+=1;
            }
        }
        eprintln!("Native click: desktop pixels changed={differences}, bright={high_contrast}");
        assert!(differences>=130,
            "Layered click painted into Win32 but was invisible in the ACTUAL composed desktop");
        assert!(high_contrast>=75,
            "Click ripple was not visible as a real cyan ring in the composed desktop");
    }
    #[test]
    fn alpha_ring_is_transparent_outside_and_visible_during_a_click(){
        assert_eq!(ring_pixel(0.,0.,0.)[3],0);
        assert!(ring_pixel(SIDE as f32/2.+24.,SIDE as f32/2.,0.)[3]>200);
        assert!(ring_pixel(SIDE as f32/2.+54.,SIDE as f32/2.,0.4)[3]>100);
        assert!(ring_pixel(SIDE as f32/2.,SIDE as f32/2.,0.6)[3]>180);
        assert!(ring_pixel(SIDE as f32/2.+52.,SIDE as f32/2.,0.4)[1]>100);
        assert_eq!(ring_pixel(SIDE as f32/2.,SIDE as f32/2.,1.)[3],0);
    }
    #[test]
    fn stored_pixels_are_premultiplied_alpha(){
        let px=blended(0.3,10,240,180);
        assert!(px[3]>0 && px[3]<255);
        assert!(px[0]<=px[3] && px[1]<=px[3] && px[2]<=px[3]);
    }
}
