//! Passive click-through iPhone/iPad silhouette around the VERIFIED AirPlay
//! receiver HWND. No frame interception, video capture or AirPlay injection.
//! The entire video aperture is physically punched out of our WebView HWND:
//! a failed transparent WebView cannot paint white over the sender's pixels.
use std::time::Duration;
use tauri::{AppHandle, Manager, WebviewUrl, WebviewWindow, WebviewWindowBuilder};
use windows::Win32::Foundation::{HWND, RECT};
use windows::Win32::Graphics::Gdi::{
    CombineRgn, CreateRoundRectRgn, CreateRectRgn, DeleteObject, GetMonitorInfoW,
    MonitorFromWindow, MONITORINFO, MONITOR_DEFAULTTONEAREST, HGDIOBJ,
    RGN_DIFF, SetWindowRgn,
};
use windows::Win32::UI::WindowsAndMessaging::{
    GetWindowRect, IsIconic, IsWindowVisible,
};

const LABEL: &str = "ios-device-bezel";

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

/// Clip ONLY the metal rim; the original SDL window stays fully interactive.
/// Success transfers ownership of 'outer' to Windows (never double free).
fn hollow_bezel(hwnd:HWND,width:i32,height:i32,inset:i32,ipad:bool,scale:f64)->Result<(),String>{
    let outer_radius=(((if ipad {24.0}else{42.0})*scale).round() as i32).max(6);
    let inner_radius=(((if ipad {13.0}else{28.0})*scale).round() as i32).max(4);
    let outer=unsafe{CreateRoundRectRgn(0,0,width+1,height+1,outer_radius*2,outer_radius*2)};
    if outer.0.is_null(){return Err("Could not allocate iOS frame region".into());}
    let inner=unsafe{CreateRoundRectRgn(
        inset,inset,width-inset+1,height-inset+1,inner_radius*2,inner_radius*2
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
    let mut last_layout:Option<(i32,i32,i32,bool,bool)>=None;
    let mut visible=false;
    let mut ticker=tokio::time::interval(Duration::from_millis(140));
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
        let mut rect=RECT::default();
        if unsafe{GetWindowRect(video,&mut rect)}.is_err()
            || rect.right<=rect.left || rect.bottom<=rect.top
            || is_fullscreen(video,rect) {
            if visible {let _=view.hide();visible=false;}
            continue;
        }
        let video_w=rect.right-rect.left;
        let video_h=rect.bottom-rect.top;
        if video_w<100 || video_h<100 || video_w>9000 || video_h>9000 {continue;}
        let short=video_w.min(video_h) as f64;
        let long=video_w.max(video_h) as f64;
        let ipad=match style.as_str(){
            "device-ipad"=>true,
            "device-iphone"=>false,
            _=>short/long>=0.62,
        };
        let landscape=video_w>video_h;
        let scale=view.scale_factor().ok().filter(|v|v.is_finite() && *v>0.5 && *v<4.5).unwrap_or(1.0);
        let inset=(((if ipad {14.0}else{20.0})*scale).round() as i32).max(8);
        let w=video_w+2*inset;
        let h=video_h+2*inset;
        let layout=(w,h,inset,ipad,landscape);
        if last_layout!=Some(layout) {
            // Region is derived from PHYSICAL pixels, not CSS logical sizes.
            if hollow_bezel(own,w,h,inset,ipad,scale).is_err(){break;}
            let _=view.eval(format!(
                "window.mirroleapIosSkin?.('{}','{}')",
                if ipad {"ipad"}else{"iphone"},
                if landscape {"landscape"}else{"portrait"}
            ));
            last_layout=Some(layout);
        }
        if crate::device_frame::place_ios_companion(
            own,video,rect.left-inset,rect.top-inset,w,h
        ).is_err(){break;}
        if !visible {
            if view.show().is_ok(){visible=true;}
        }
    }
    if app.get_webview_window(LABEL).and_then(|v|v.hwnd().ok())
        .map(|hwnd|hwnd.0 as usize)==Some(own_raw){
        let _=view.close();
    }
}
