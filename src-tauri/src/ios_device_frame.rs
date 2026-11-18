//! Passive click-through iPhone/iPad silhouette around the VERIFIED AirPlay
//! receiver HWND. No frame interception, video capture or AirPlay injection.
//! The entire video aperture is physically punched out of our WebView HWND:
//! a failed transparent WebView cannot paint white over the sender's pixels.
use std::time::{Duration,Instant};
use tauri::{AppHandle, Manager, WebviewUrl, WebviewWindow, WebviewWindowBuilder};
use windows::Win32::Foundation::{HWND, POINT, RECT};
use windows::Win32::Graphics::Gdi::{
    ClientToScreen, CombineRgn, CreateRoundRectRgn, CreateRectRgn, DeleteObject,
    GetMonitorInfoW,
    MonitorFromWindow, MONITORINFO, MONITOR_DEFAULTTONEAREST, HGDIOBJ,
    RGN_DIFF, SetWindowRgn,
};
use windows::Win32::UI::HiDpi::GetDpiForWindow;
use windows::Win32::UI::WindowsAndMessaging::{
    GetClientRect, GetWindowRect, IsIconic, IsWindowVisible,
};

const LABEL: &str = "ios-device-bezel";

#[derive(Clone,Copy,Debug,PartialEq,Eq)]
struct PixelRect {
    x:i32,y:i32,width:i32,height:i32,
}
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
struct ShellMetrics {
    outer:PixelRect,
    client:PixelRect,
}
fn pixel_aligned(actual:PixelRect,target:PixelRect)->bool {
    (actual.x-target.x).abs()<=1
        && (actual.y-target.y).abs()<=1
        && (actual.width-target.width).abs()<=1
        && (actual.height-target.height).abs()<=1
}
/// The previous implementation used GetWindowRect of the foreign SDL HWND,
/// which includes an invisible Windows resize frame. It also sized this
/// Tauri WebView using the *outer* dimensions, ignoring its own non-client
/// offsets. Both errors compounded after Windows DPI scaling / resizing.
/// Use Android's already-proven GetClientRect + ClientToScreen contract.
fn screen_client(hwnd:HWND)->Result<PixelRect,String>{
    let mut bounds=RECT::default();
    unsafe{GetClientRect(hwnd,&mut bounds)}
        .map_err(|e|format!("Cannot read video client area: {e}"))?;
    let mut origin=POINT::default();
    if !unsafe{ClientToScreen(hwnd,&mut origin)}.as_bool(){
        return Err("Cannot locate video client origin".into());
    }
    let width=bounds.right-bounds.left;
    let height=bounds.bottom-bounds.top;
    if width<=0 || height<=0 {return Err("Empty video client area".into());}
    Ok(PixelRect{x:origin.x,y:origin.y,width,height})
}
pub(crate) fn receiver_client_size(pid:u32)->Option<(u32,u32)> {
    let hwnd=crate::commands::find_window_by_pid(pid)?;
    let rc=screen_client(hwnd).ok()?;
    if rc.width<100 || rc.height<100 {return None;}
    Some((rc.width as u32,rc.height as u32))
}
fn shell_metrics(hwnd:HWND)->Result<ShellMetrics,String>{
    let client=screen_client(hwnd)?;
    let mut bounds=RECT::default();
    unsafe{GetWindowRect(hwnd,&mut bounds)}
        .map_err(|e|format!("Cannot read iOS bezel HWND bounds: {e}"))?;
    Ok(ShellMetrics{
        outer:PixelRect{x:bounds.left,y:bounds.top,
            width:bounds.right-bounds.left,height:bounds.bottom-bounds.top},
        client,
    })
}
/// Solve for the native OUTER HWND placement that puts the WebView CLIENT
/// exactly around the video CLIENT, including invisible Win32 frame margins.
fn outer_for_client(target:PixelRect, shell:ShellMetrics)->Option<PixelRect>{
    let left_inset=shell.client.x.checked_sub(shell.outer.x)?;
    let top_inset=shell.client.y.checked_sub(shell.outer.y)?;
    let extra_w=shell.outer.width.checked_sub(shell.client.width)?;
    let extra_h=shell.outer.height.checked_sub(shell.client.height)?;
    if left_inset.abs()>64 || top_inset.abs()>64 || extra_w.abs()>128 || extra_h.abs()>128 {
        return None;
    }
    Some(PixelRect{
        x:target.x.checked_sub(left_inset)?,
        y:target.y.checked_sub(top_inset)?,
        width:target.width.checked_add(extra_w)?,
        height:target.height.checked_add(extra_h)?,
    })
}

pub fn close(app:&AppHandle) {
    if let Some(old)=app.get_webview_window(LABEL) {let _=old.close();}
}

pub fn start(app:&AppHandle,pid:u32)->Result<(),String>{
    close(app);
    let view=WebviewWindowBuilder::new(app,LABEL,WebviewUrl::App("ios-bezel.html".into()))
        .title("MirroLeap iOS bezel")
        .background_color(tauri::window::Color(0,0,0,0))
        .transparent(true)
        .decorations(false)
        .shadow(false)
        .resizable(false)
        .skip_taskbar(true)
        .focused(false)
        .visible(false)
        .inner_size(440.0,780.0)
        .build()
        .map_err(|e|format!("Cannot create iOS device bezel: {e}"))?;
    if let Err(error)=view.set_ignore_cursor_events(true) {
        let _=view.close();
        return Err(format!("iOS frame must be mouse-through: {error}"));
    }
    tokio::spawn(follow(app.clone(),view,pid));
    Ok(())
}

fn is_fullscreen(video:HWND,rect:RECT)->bool{
    let monitor=unsafe{MonitorFromWindow(video,MONITOR_DEFAULTTONEAREST)};
    if monitor.0.is_null(){return false;}
    let mut info=MONITORINFO{cbSize:std::mem::size_of::<MONITORINFO>() as u32,..Default::default()};
    if !unsafe{GetMonitorInfoW(monitor,&mut info)}.as_bool(){return false;}
    let screen=info.rcMonitor;
    rect.left<=screen.left+4 && rect.top<=screen.top+4
        && rect.right>=screen.right-4 && rect.bottom>=screen.bottom-4
}

/// Cut out the EXACT SDL CLIENT area rather than estimating the screen hole
/// from the foreign window's decorated bounding box. All coordinates are
/// relative to the bezel OUTER HWND, not the WebView's CSS origin.
fn hollow_bezel(
    hwnd:HWND, shell:ShellMetrics, video:PixelRect, ipad:bool,scale:f64,
)->Result<(),String>{
    let outer_radius=(((if ipad {27.0}else{40.0})*scale).round() as i32).max(6);
    let inner_radius=(((if ipad {16.0}else{27.0})*scale).round() as i32).max(4);
    let left=shell.client.x-shell.outer.x;
    let top=shell.client.y-shell.outer.y;
    let width=shell.client.width;
    let height=shell.client.height;
    let outer=unsafe{CreateRoundRectRgn(left,top,
        left+width+1,top+height+1,outer_radius*2,outer_radius*2)};
    if outer.0.is_null(){return Err("Could not allocate iOS frame region".into());}
    let hole_left=video.x-shell.outer.x;
    let hole_top=video.y-shell.outer.y;
    let inner=unsafe{CreateRoundRectRgn(
        hole_left,hole_top,hole_left+video.width+1,hole_top+video.height+1,
        inner_radius*2,inner_radius*2
    )};
    let rim=unsafe{CreateRectRgn(0,0,0,0)};
    let ok=!inner.0.is_null() && !rim.0.is_null()
        && unsafe{CombineRgn(Some(rim),Some(outer),Some(inner),RGN_DIFF)}.0>=2;
    let _=unsafe{DeleteObject(HGDIOBJ(outer.0))};
    if !inner.0.is_null(){let _=unsafe{DeleteObject(HGDIOBJ(inner.0))};}
    if !ok {
        if !rim.0.is_null(){let _=unsafe{DeleteObject(HGDIOBJ(rim.0))};}
        return Err("Could not create the iOS bezel aperture".into());
    }
    if unsafe{SetWindowRgn(hwnd,Some(rim),true)}==0 {
        let _=unsafe{DeleteObject(HGDIOBJ(rim.0))};
        return Err("Windows rejected iOS bezel region".into());
    }
    Ok(())
}

async fn follow(app:AppHandle,view:WebviewWindow,pid:u32) {
    // Leave the window hidden until WebView2 has loaded its transparent page.
    tokio::time::sleep(Duration::from_millis(450)).await;
    // Keep only a Send-safe integer across tokio awaits, not an HWND pointer.
    let own_raw=match view.hwnd(){Ok(hwnd)=>hwnd.0 as usize,Err(_)=>return};
    let mut last_layout:Option<(i32,i32,i32,bool,bool,u32,i32,i32)>=None;
    let mut visible=false;
    let mut stacked_at=Instant::now()-Duration::from_secs(1);
    let mut style_sent_at=Instant::now()-Duration::from_secs(1);
    let mut failed_geometry_logged=false;
    let mut ticker=tokio::time::interval(Duration::from_millis(50));
    ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    loop {
        ticker.tick().await;
        if app.get_webview_window(LABEL).and_then(|v|v.hwnd().ok())
            .map(|hwnd|hwnd.0 as usize)!=Some(own_raw){break;}
        let own=HWND(own_raw as *mut _);
        let active_pid=app.state::<crate::ios_airplay::IosAirplayState>()
            .child.lock().ok().and_then(|c|c.as_ref().map(|p|p.id()));
        if active_pid!=Some(pid){break;}
        let style=app.state::<crate::ios_airplay::IosAirplayState>()
            .window_style.lock().ok().map(|s|s.clone()).unwrap_or_default();
        let video=crate::commands::find_window_by_pid(pid);
        let Some(video)=video else {
            if visible {let _=view.hide();visible=false;}
            // The sender may connect much later than the receiver starts.
            // Keep the transparent companion hidden, never time it out.
            continue;
        };
        if !style.starts_with("device-")
            || !unsafe{IsWindowVisible(video)}.as_bool()
            || unsafe{IsIconic(video)}.as_bool() {
            if visible {let _=view.hide();visible=false;}
            continue;
        }
        // The actual SDL CLIENT is our source of truth, as in Android's
        // native device-frame follower. GetWindowRect includes a hidden
        // WS_THICKFRAME resize margin on the upstream borderless receiver.
        let Ok(screen)=screen_client(video) else {
            if visible {let _=view.hide();visible=false;}
            continue;
        };
        if is_fullscreen(video,RECT{left:screen.x,top:screen.y,
            right:screen.x+screen.width,bottom:screen.y+screen.height}){
            if visible {let _=view.hide();visible=false;}
            continue;
        }
        let video_w=screen.width;
        let video_h=screen.height;
        if video_w<100 || video_h<100 || video_w>9000 || video_h>9000 {continue;}
        let short=video_w.min(video_h) as f64;
        let long=video_w.max(video_h) as f64;
        let ipad=match style.as_str(){
            "device-ipad"=>true,
            "device-iphone"=>false,
            _=>short/long>=0.62,
        };
        let landscape=video_w>video_h;
        let dpi=unsafe{GetDpiForWindow(own)}.clamp(96,384);
        let scale=f64::from(dpi)/96.0;
        let inset=(((if ipad {16.0}else{18.0})*scale).round() as i32).max(8);
        let Some(w)=video_w.checked_add(2*inset) else {continue};
        let Some(h)=video_h.checked_add(2*inset) else {continue};
        let target=PixelRect{x:screen.x-inset,y:screen.y-inset,width:w,height:h};
        let Ok(mut metrics)=shell_metrics(own) else {continue};
        let mut aligned=pixel_aligned(metrics.client,target);
        // Re-stack periodically when Alt+Tab changes tiers; avoid endlessly
        // resizing the same HWND on every video frame.
        if !aligned || stacked_at.elapsed()>Duration::from_millis(450) {
            let Some(outer)=outer_for_client(target,metrics) else {continue};
            if crate::device_frame::place_ios_companion(
                own,video,outer.x,outer.y,outer.width,outer.height
            ).is_err(){break;}
            stacked_at=Instant::now();
            if let Ok(next)=shell_metrics(own){
                metrics=next;
                aligned=pixel_aligned(metrics.client,target);
                if !aligned {
                    // One correction for a late Win32 non-client offset/DPI
                    // adjustment. Never flash an incorrectly placed bezel.
                    if let Some(retry)=outer_for_client(target,metrics) {
                        let _=crate::device_frame::place_ios_companion(
                            own,video,retry.x,retry.y,retry.width,retry.height);
                        if let Ok(next)=shell_metrics(own){
                            metrics=next;
                            aligned=pixel_aligned(metrics.client,target);
                        }
                    }
                }
            }
        }
        if !aligned {
            if visible {let _=view.hide();visible=false;}
            if !failed_geometry_logged {
                eprintln!("[iOS frame] Client geometry differs from AirPlay video; bezel hidden");
                failed_geometry_logged=true;
            }
            last_layout=None;
            continue;
        }
        failed_geometry_logged=false;
        // DWM can change the invisible non-client offset without resizing
        // the actual WebView; the native region must move with that inset.
        let layout=(w,h,inset,ipad,landscape,dpi,
            metrics.client.x-metrics.outer.x,metrics.client.y-metrics.outer.y);
        if last_layout!=Some(layout) {
            // Only apply the GDI aperture AFTER a successful HWND resize.
            if hollow_bezel(own,metrics,screen,ipad,scale).is_err(){break;}
            last_layout=Some(layout);
            style_sent_at=Instant::now()-Duration::from_secs(1);
        }
        if style_sent_at.elapsed()>Duration::from_millis(900){
            let _=view.eval(format!(
                "window.mirroleapIosSkin?.('{}','{}',{})",
                if ipad {"ipad"}else{"iphone"},
                if landscape {"landscape"}else{"portrait"},
                inset as f64/scale
            ));
            style_sent_at=Instant::now();
        }
        if !visible {
            if view.show().is_ok(){visible=true;}
        }
    }
    if app.get_webview_window(LABEL).and_then(|v|v.hwnd().ok())
        .map(|hwnd|hwnd.0 as usize)==Some(own_raw){
        let _=view.close();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rect(x:i32,y:i32,width:i32,height:i32)->PixelRect{
        PixelRect{x,y,width,height}
    }

    #[test]
    fn moves_bezel_by_video_client_not_resizable_outer_sdl_window(){
        // SDL outer starts 8px before its actual screen content. The shell's
        // own invisible frame is 7px and must be compensated separately.
        let video_client=rect(145,59,865,1246);
        let rim=16;
        let expected=rect(video_client.x-rim,video_client.y-rim,
            video_client.width+2*rim,video_client.height+2*rim);
        let shell=ShellMetrics {
            outer:rect(85,40,440,780),
            client:rect(93,48,424,764),
        };
        let result=outer_for_client(expected,shell).unwrap();
        assert_eq!(result,rect(121,35,913,1294));
        let observed=ShellMetrics {
            outer:result,
            client:expected,
        };
        assert!(pixel_aligned(observed.client,expected));
    }

    #[test]
    fn handles_dpi_and_negative_monitor_coordinates_without_drift(){
        for (x,y,margin) in [(-1480,-270,0),(100,80,8),(1900,300,12)] {
            let client=rect(x,y,1100,840);
            let shell=ShellMetrics{
                outer:rect(x-margin,y-margin,1100+margin*2,840+margin*2),
                client,
            };
            let target=rect(x-20,y-20,1140,880);
            let outer=outer_for_client(target,shell).unwrap();
            assert_eq!(outer.x+margin,target.x);
            assert_eq!(outer.y+margin,target.y);
            assert_eq!(outer.width-2*margin,target.width);
            assert_eq!(outer.height-2*margin,target.height);
        }
        assert!(pixel_aligned(rect(0,0,100,200),rect(1,-1,101,199)));
        assert!(!pixel_aligned(rect(0,0,100,200),rect(10,0,100,200)));
    }

    #[test]
    fn refuses_impossible_nonclient_margins(){
        assert!(outer_for_client(rect(10,10,300,400),ShellMetrics{
            outer:rect(0,0,20,20),
            client:rect(200,200,5,5),
        }).is_none());
    }
}
