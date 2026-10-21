//! Experimental, opt-in **source-decoded-frame** Windows preview backend.
//!
//! Stage 3 reference: consume BGRA generated from the *original decoded*
//! video frame and present it in a click-through, non-activating layered
//! round HWND. This intentionally does NOT hook into normal MirroLeap app
//! sessions: without a proven custom scrcpy producer it must stay dormant.
//!
//! This is a bounded CPU/GDI diagnostic presenter, NOT the final D3D11
//! zero-copy GPU renderer. It is useful to demonstrate source-pixel fidelity
//! and verify window lifecycle before enabling more costly GPU integration.

#![cfg(target_os = "windows")]

use std::ffi::c_void;
use std::ptr::{self, NonNull};
use crate::hq_bridge_windows::HqMappingOwner;
use crate::hq_geometry::ScreenViewport;
use crate::hq_pipeline::render_decoded_hq_preview;

const MAX_LENS:u32 = 720;
const EX_LAYERED:u32 = 0x0008_0000;
const EX_TRANSPARENT:u32 = 0x0000_0020;
const EX_TOOLWINDOW:u32 = 0x0000_0080;
const EX_NOACTIVATE:u32 = 0x0800_0000;
const EX_TOPMOST:u32 = 0x0000_0008;
const WS_POPUP:u32 = 0x8000_0000;
const ULW_ALPHA:u32 = 0x02;
const AC_SRC_ALPHA:u8 = 0x01;
const DIB_RGB_COLORS:u32 = 0;

#[repr(C)]
#[derive(Copy,Clone)]
struct Point {x:i32,y:i32}
#[repr(C)]
#[derive(Copy,Clone)]
struct Size {cx:i32,cy:i32}
#[repr(C)]
#[derive(Copy,Clone)]
struct Blend {
    operation:u8, flags:u8, constant_alpha:u8, alpha_format:u8,
}
#[repr(C)]
#[derive(Copy,Clone)]
struct BitmapInfoHeader {
    size:u32,width:i32,height:i32,planes:u16,bits_per_pixel:u16,
    compression:u32,size_image:u32,ppm_x:i32,ppm_y:i32,
    colors_used:u32,colors_important:u32,
}
#[repr(C)]
struct BitmapInfo {
    header:BitmapInfoHeader,
    colors:[u32;1],
}

#[link(name="user32")]
unsafe extern "system" {
    fn CreateWindowExW(ex_style:u32,class_name:*const u16,title:*const u16,
        style:u32,x:i32,y:i32,w:i32,h:i32,parent:*mut c_void,menu:*mut c_void,
        instance:*mut c_void,params:*mut c_void)->*mut c_void;
    fn DestroyWindow(hwnd:*mut c_void)->i32;
    fn ShowWindow(hwnd:*mut c_void,command:i32)->i32;
    fn GetDC(hwnd:*mut c_void)->*mut c_void;
    fn ReleaseDC(hwnd:*mut c_void,dc:*mut c_void)->i32;
    fn UpdateLayeredWindow(hwnd:*mut c_void,dst_dc:*mut c_void,
        target:*const Point,size:*const Size,source_dc:*mut c_void,
        src_point:*const Point,color_key:u32,blend:*const Blend,flags:u32)->i32;
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

fn wide(value:&str)->Vec<u16> {value.encode_utf16().chain(Some(0)).collect()}

/// Premultiplied BGRA circular mask. Antialiasing is only applied at the
/// outer circle boundary; all interior pixels preserve their decoded-source
/// BGRA values without a second Windows Magnification pass.
pub(crate) fn circular_premultiplied_bgra(
    bgra:&[u8],diameter:u32,
)->Result<Vec<u8>,&'static str> {
    if !(40..=MAX_LENS).contains(&diameter) {
        return Err("HQ circle diameter outside reviewed bounds");
    }
    let size=diameter as usize;
    let expected=size.checked_mul(size)
        .and_then(|area|area.checked_mul(4)).ok_or("HQ circle size overflow")?;
    if bgra.len()!=expected {return Err("HQ source BGRA length mismatch");}
    let radius=diameter as f32*0.5;
    let mut result=vec![0u8;expected];
    for y in 0..size {
        let dy=(y as f32+0.5)-radius;
        for x in 0..size {
            let dx=(x as f32+0.5)-radius;
            let distance=(dx*dx+dy*dy).sqrt();
            let coverage=(radius-distance).clamp(0.0,1.0);
            if coverage==0.0 {continue;}
            let pos=(y*size+x)*4;
            let alpha=(bgra[pos+3] as f32*coverage).round() as u8;
            for component in 0..3 {
                result[pos+component]=
                    ((bgra[pos+component] as u16 * alpha as u16+127)/255) as u8;
            }
            result[pos+3]=alpha;
        }
    }
    Ok(result)
}

/// Use 64-bit Windows coordinates to guard against malformed/excessive
/// viewport dimensions and prevent clamping with inverted bounds.
pub(crate) fn lens_position(
    viewport:ScreenViewport,cursor:(i32,i32),diameter:u32,
)->Result<(i32,i32),&'static str> {
    if !(40..=MAX_LENS).contains(&diameter) {
        return Err("HQ circle diameter outside bounds");
    }
    let w=viewport.width as i64;
    let h=viewport.height as i64;
    if w<diameter as i64+8 || h<diameter as i64+8 ||
        w>65535 || h>65535 {
        return Err("HQ circle does not fit the viewport");
    }
    let left=viewport.x as i64;
    let top=viewport.y as i64;
    if (cursor.0 as i64)<left || (cursor.0 as i64)>=left+w ||
       (cursor.1 as i64)<top || (cursor.1 as i64)>=top+h {
        return Err("HQ cursor is not inside the phone viewport");
    }
    let radius=diameter as i64/2;
    let x=((cursor.0 as i64)-radius).clamp(left+4,left+w-diameter as i64-4);
    let y=((cursor.1 as i64)-radius).clamp(top+4,top+h-diameter as i64-4);
    Ok((
        i32::try_from(x).map_err(|_|"HQ window X outside desktop bounds")?,
        i32::try_from(y).map_err(|_|"HQ window Y outside desktop bounds")?,
    ))
}

/// Redraw a cached decoded source frame when the cursor, viewport, circle
/// size or optical zoom changes, even if scrcpy does not emit a new frame.
/// Otherwise a paused video makes the HQ lens appear frozen while dragging.
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
struct PresentationKey {
    publication:u32,
    viewport:(i32,i32,u32,u32),
    cursor:(i32,i32),
    diameter:u32,
    zoom_bits:u32,
}
impl PresentationKey {
    fn new(
        publication:u32,viewport:ScreenViewport,cursor:(i32,i32),
        diameter:u32,zoom:f32,
    )->Self {
        Self {
            publication,
            viewport:(viewport.x,viewport.y,viewport.width,viewport.height),
            cursor,diameter,zoom_bits:zoom.to_bits(),
        }
    }
}

/// A single-thread-owned diagnostic output window. The caller must create,
/// update and drop this from one GUI/render worker thread with a message pump.
pub struct HqDecodedCpuLens {
    hwnd:NonNull<c_void>,
    dc:NonNull<c_void>,
    default_bitmap:NonNull<c_void>,
    bitmap:Option<NonNull<c_void>>,
    pixels:Option<NonNull<u8>>,
    dimension:u32,
    previous_key:Option<PresentationKey>,
    cached_frame:Option<(u32,crate::hq_protocol::OwnedFrame)>,
    last_generation:u64,
}

impl HqDecodedCpuLens {
    pub fn create()->Result<Self,&'static str> {
        let class=wide("STATIC");
        let title=wide("MirroLeap HQ decoded video reference lens");
        let raw=unsafe {
            CreateWindowExW(
                EX_LAYERED|EX_TRANSPARENT|EX_TOOLWINDOW|EX_NOACTIVATE|EX_TOPMOST,
                class.as_ptr(),title.as_ptr(),WS_POPUP,0,0,1,1,
                ptr::null_mut(),ptr::null_mut(),ptr::null_mut(),ptr::null_mut())
        };
        let Some(hwnd)=NonNull::new(raw) else {
            return Err("Windows could not create the experimental HQ window");
        };
        let dc=unsafe {CreateCompatibleDC(ptr::null_mut())};
        let Some(dc)=NonNull::new(dc) else {
            unsafe {DestroyWindow(hwnd.as_ptr());}
            return Err("Windows could not create the HQ memory surface");
        };
        Ok(Self {
            hwnd,dc,
            default_bitmap:NonNull::dangling(),
            bitmap:None,pixels:None,dimension:0,previous_key:None,cached_frame:None,
            last_generation:0,
        })
    }

    fn allocate_surface(&mut self,diameter:u32)->Result<(),&'static str> {
        if self.dimension==diameter && self.bitmap.is_some() {return Ok(());}
        if !(40..=MAX_LENS).contains(&diameter) {
            return Err("HQ surface size outside reviewed bounds");
        }
        let bitmap_info=BitmapInfo {
            header:BitmapInfoHeader {
                size:std::mem::size_of::<BitmapInfoHeader>() as u32,
                width:diameter as i32,height:-(diameter as i32),
                planes:1,bits_per_pixel:32,compression:0,
                size_image:diameter*diameter*4,
                ppm_x:0,ppm_y:0,colors_used:0,colors_important:0,
            },
            colors:[0],
        };
        let mut bits=ptr::null_mut();
        let bitmap=unsafe {
            CreateDIBSection(ptr::null_mut(),&bitmap_info,DIB_RGB_COLORS,
                &mut bits,ptr::null_mut(),0)
        };
        let Some(new_bitmap)=NonNull::new(bitmap) else {
            return Err("Could not allocate HQ BGRA DIB");
        };
        let Some(new_pixels)=NonNull::new(bits as *mut u8) else {
            unsafe {DeleteObject(new_bitmap.as_ptr());}
            return Err("HQ BGRA DIB returned no pixels");
        };
        let selected=unsafe {SelectObject(self.dc.as_ptr(),new_bitmap.as_ptr())};
        // GDI returns NULL or HGDI_ERROR (-1) when selecting fails.
        let Some(previous)=NonNull::new(selected).filter(|object|
            object.as_ptr() as isize != -1) else {
            unsafe {DeleteObject(new_bitmap.as_ptr());}
            return Err("Could not select HQ BGRA DIB");
        };
        if self.bitmap.is_none() {self.default_bitmap=previous;}
        if let Some(old)=self.bitmap.replace(new_bitmap) {
            unsafe {DeleteObject(old.as_ptr());}
        }
        self.pixels=Some(new_pixels);
        self.dimension=diameter;
        Ok(())
    }

    fn present_bgra(&mut self,bgra:&[u8],x:i32,y:i32,diameter:u32)
        ->Result<(),&'static str>
    {
        let masked=circular_premultiplied_bgra(bgra,diameter)?;
        self.allocate_surface(diameter)?;
        let Some(bits)=self.pixels else {return Err("HQ BGRA surface missing");};
        unsafe {ptr::copy_nonoverlapping(masked.as_ptr(),bits.as_ptr(),masked.len());}
        let point=Point{x,y};
        let size=Size{cx:diameter as i32,cy:diameter as i32};
        let source=Point{x:0,y:0};
        let blend=Blend {
            operation:0,flags:0,constant_alpha:255,alpha_format:AC_SRC_ALPHA,
        };
        let screen=unsafe{GetDC(ptr::null_mut())};
        let Some(screen)=NonNull::new(screen) else {
            return Err("Windows desktop DC unavailable for HQ presentation");
        };
        let ok=unsafe {
            UpdateLayeredWindow(self.hwnd.as_ptr(),screen.as_ptr(),&point,
                &size,self.dc.as_ptr(),&source,0,&blend,ULW_ALPHA)
        };
        unsafe {ReleaseDC(ptr::null_mut(),screen.as_ptr());}
        if ok==0 {
            return Err("Windows could not update experimental HQ round lens");
        }
        unsafe {ShowWindow(self.hwnd.as_ptr(),4);} // SW_SHOWNOACTIVATE
        Ok(())
    }

    pub fn hide(&mut self) {
        unsafe {ShowWindow(self.hwnd.as_ptr(),0);}
        self.previous_key=None;
    }

    /// True only after a complete valid source frame was rendered.
    pub fn is_presenting(&self)->bool {self.previous_key.is_some()}

    pub fn reset_source(&mut self) {
        self.cached_frame=None;
        self.hide();
    }

    /// Opt-in source-preview adapter. Inactive or unchanged frames never
    /// repaint the HWND. Never call from scrcpy's decoder thread.
    ///
    /// Staging requires real source coordinates and current device identity
    /// from the caller. A missing/malformed frame removes stale output.
    pub fn update_from_source(
        &mut self, owner:&HqMappingOwner, viewport:ScreenViewport,
        cursor:(i32,i32),lens_diameter:u32,zoom:f32,
    )->Result<bool,&'static str> {
        let generation=owner.generation();
        if generation!=self.last_generation {
            // The publish counter restarts from 1 on decoder/audio retry.
            // Never reuse a cached frame with the same ID from the OLD stream.
            self.reset_source();
            self.last_generation=generation;
        }
        let publish_id=match owner.latest_publish_id() {
            Ok(Some(id))=>id,
            Ok(None)=>{self.reset_source();return Ok(false);},
            Err(error)=>{self.reset_source();return Err(error);},
        };
        let key=PresentationKey::new(
            publish_id,viewport,cursor,lens_diameter,zoom);
        // Paused/static Android images keep full source resolution as the
        // pointer moves, without allocating/copying ~12 MiB every idle tick.
        if self.previous_key==Some(key) {return Ok(false);}
        if self.cached_frame.as_ref().map(|(id,_)|*id)!=Some(publish_id) {
            let next=match owner.try_latest_with_publish_id() {
                Ok(frame)=>frame,
                Err(error)=>{self.reset_source();return Err(error);},
            };
            let Some(frame)=next else {return Ok(false);};
            self.cached_frame=Some(frame);
        }
        let (left,top)=lens_position(viewport,cursor,lens_diameter)
            .inspect_err(|_|self.hide())?;
        let frame=&self.cached_frame.as_ref().expect("HQ source cached").1;
        let image=render_decoded_hq_preview(
            frame,viewport,cursor,lens_diameter,zoom)
            .inspect_err(|_|self.hide())?;
        self.present_bgra(&image.bgra,left,top,lens_diameter)
            .inspect_err(|_|self.hide())?;
        self.previous_key=Some(key);
        Ok(true)
    }
}
impl Drop for HqDecodedCpuLens {
    fn drop(&mut self) {
        unsafe {
            ShowWindow(self.hwnd.as_ptr(),0);
            if self.bitmap.is_some() {
                SelectObject(self.dc.as_ptr(),self.default_bitmap.as_ptr());
            }
            if let Some(bitmap)=self.bitmap.take() {
                DeleteObject(bitmap.as_ptr());
            }
            DeleteDC(self.dc.as_ptr());
            DestroyWindow(self.hwnd.as_ptr());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn lens_redraws_for_cursor_or_zoom_even_when_video_frame_is_unchanged() {
        let viewport=ScreenViewport{x:20,y:30,width:360,height:640};
        let initial=PresentationKey::new(2,viewport,(130,150),180,2.35);
        let same=PresentationKey::new(2,viewport,(130,150),180,2.35);
        assert_eq!(initial,same);
        assert_ne!(initial,PresentationKey::new(3,viewport,(130,150),180,2.35));
        assert_ne!(initial,PresentationKey::new(2,viewport,(131,150),180,2.35));
        assert_ne!(initial,PresentationKey::new(2,viewport,(130,150),200,2.35));
        assert_ne!(initial,PresentationKey::new(2,viewport,(130,150),180,1.55));
        assert_ne!(initial,PresentationKey::new(2,ScreenViewport{x:21,..viewport},
                                                (130,150),180,2.35));
    }

    #[test]
    fn round_bgra_is_transparent_outside_and_keeps_source_center_pixels() {
        let mut pixels=vec![40u8;100*100*4];
        for pixel in pixels.chunks_exact_mut(4) {pixel[3]=255;}
        let output=circular_premultiplied_bgra(&pixels,100).unwrap();
        assert_eq!(output.len(),100*100*4);
        assert_eq!(&output[..4],&[0,0,0,0]);
        let center=(50*100+50)*4;
        assert_eq!(&output[center..center+4],&[40,40,40,255]);
        let top=(0*100+50)*4;
        assert!(output[top+3]>0 && output[top+3]<255);
        assert!(output.chunks_exact(4).any(|pixel|pixel[3]>0 && pixel[3]<255));
    }
    #[test]
    fn native_layered_window_can_present_a_bgra_circle_when_opted_into_smoke() {
        if std::env::var_os("MIRROLEAP_HQ_GDI_SMOKE").is_none() {
            // Some headless Windows builders cannot provide a composited
            // desktop. CI enables this only after Edge GUI smoke succeeds.
            return;
        }
        let mut window=HqDecodedCpuLens::create()
            .expect("Windows user32 window and GDI HDC");
        let mut bgra=vec![0u8;128*128*4];
        for pixel in bgra.chunks_exact_mut(4) {
            pixel.copy_from_slice(&[20,165,225,255]);
        }
        window.present_bgra(&bgra,40,40,128)
            .expect("top-down BGRA DIB and native layered window rendering");
        window.hide();
        window.present_bgra(&bgra,42,43,128)
            .expect("redraw after a temporary hide");
        window.hide();
        // Drop must restore selected stock bitmap and release HDC/HBITMAP/HWND.
    }
    #[test]
    fn round_bgra_checks_dimensions_before_access() {
        for size in [0,39,721,10_000] {
            assert!(circular_premultiplied_bgra(&[],size).is_err());
        }
        assert!(circular_premultiplied_bgra(&[0;100],100).is_err());
    }
    #[test]
    fn lens_placement_respects_all_edges_and_refuses_invalid_cursor() {
        let viewport=ScreenViewport{x:-310,y:42,width:360,height:640};
        for (x,y) in [
            (-310,42), (49,681),(-310,681),(49,42),
            (-130,362),
        ] {
            let (left,top)=lens_position(viewport,(x,y),240).unwrap();
            assert!(left>=viewport.x+4);
            assert!(top>=viewport.y+4);
            assert!(left+240<=viewport.x+viewport.width as i32-4);
            assert!(top+240<=viewport.y+viewport.height as i32-4);
        }
        assert!(lens_position(viewport,(-311,400),240).is_err());
        assert!(lens_position(viewport,(50,400),240).is_err());
        assert!(lens_position(viewport,(0,300),500).is_err());
        assert!(lens_position(viewport,(0,300),u32::MAX).is_err());
        assert!(lens_position(ScreenViewport{x:i32::MAX,y:0,width:400,height:400},
                (i32::MAX,100),200).is_err());
    }

    #[test]
    fn central_full_alpha_is_not_modified_and_preserves_bgra_order() {
        let mut bgra=vec![0u8;64*64*4];
        bgra.chunks_exact_mut(4).for_each(|pixel|pixel.copy_from_slice(&[200,140,90,255]));
        let out=circular_premultiplied_bgra(&bgra,64).unwrap();
        let center=(32*64+32)*4;
        assert_eq!(&out[center..center+4],&[200,140,90,255]);
        assert_eq!(&out[..4],&[0,0,0,0]);
    }
}
