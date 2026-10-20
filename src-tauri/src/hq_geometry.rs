//! Source-pixel mapping for the experimental decoded-frame HQ magnifier.
//! A 2.35× zoom should sample decoded phone pixels *before* the native SDL
//! mirror downsamples them to a small Windows client rectangle.
//!
//! Pure math only: no GPU resources, device IO or extra scrcpy streams.

#[derive(Clone,Copy,Debug,PartialEq)]
pub struct ScreenViewport {
    pub x: i32,
    pub y: i32,
    pub width: u32,   // physical Windows pixels, not CSS pixels
    pub height: u32,
}

#[derive(Clone,Copy,Debug,PartialEq)]
pub struct SourceFrame {
    pub width: u32,   // true decoded AVFrame dimensions
    pub height: u32,
    pub rotation: u32, // 0,90,180,270 clockwise; 0 for already-oriented frames
}

#[derive(Clone,Copy,Debug,PartialEq)]
pub struct SourceSample {
    pub source_center_x: f32,
    pub source_center_y: f32,
    pub source_left: f32,
    pub source_top: f32,
    pub source_width: f32,
    pub source_height: f32,
    pub valid_left: f32,
    pub valid_top: f32,
    pub valid_right: f32,
    pub valid_bottom: f32,
    pub lens_diameter_px: u32,
    pub zoom: f32,
}

pub fn map_lens_to_decoded_frame(
    frame:SourceFrame, viewport:ScreenViewport, cursor:(i32,i32),
    lens_diameter_px:u32, zoom:f32,
)->Result<SourceSample,&'static str> {
    if frame.width<2 || frame.height<2 ||
        frame.width>3840 || frame.height>3840 ||
        (frame.width as u64)*(frame.height as u64)>3840u64*2160u64 {
        return Err("source size outside reviewed HQ frame bounds");
    }
    if viewport.width<64 || viewport.height<64 || lens_diameter_px<40 ||
        !zoom.is_finite() || !(1.0..=3.5).contains(&zoom) {
        return Err("invalid HQ viewport or magnification");
    }
    if !matches!(frame.rotation,0|90|180|270) {
        return Err("unsupported HQ decoded frame rotation");
    }
    let cw=viewport.width as i64;
    let ch=viewport.height as i64;
    let x=(cursor.0 as i64)-(viewport.x as i64);
    let y=(cursor.1 as i64)-(viewport.y as i64);
    if x<0 || y<0 || x>=cw || y>=ch {
        return Err("HQ cursor outside actual video viewport");
    }
    let nx=x as f32/cw as f32;
    let ny=y as f32/ch as f32;
    let fw=frame.width as f32;
    let fh=frame.height as f32;
    let (sx,sy)=match frame.rotation {
        0=>(nx*fw,ny*fh),
        90=>(ny*fw,(1.0-nx)*fh),
        180=>((1.0-nx)*fw,(1.0-ny)*fh),
        270=>((1.0-ny)*fw,nx*fh),
        _=>unreachable!(),
    };
    let rotated=frame.rotation==90 || frame.rotation==270;
    let video_w=if rotated {fh} else {fw};
    let video_h=if rotated {fw} else {fh};
    let width_in_view= lens_diameter_px as f32 / zoom *
        video_w / viewport.width as f32;
    let height_in_view= lens_diameter_px as f32 / zoom *
        video_h / viewport.height as f32;
    let (source_width,source_height)=if rotated {
        (height_in_view,width_in_view)
    } else {
        (width_in_view,height_in_view)
    };
    let left=sx-source_width/2.0;
    let top=sy-source_height/2.0;
    let right=left+source_width;
    let bottom=top+source_height;
    let valid_left=left.clamp(0.0,fw);
    let valid_right=right.clamp(0.0,fw);
    let valid_top=top.clamp(0.0,fh);
    let valid_bottom=bottom.clamp(0.0,fh);
    Ok(SourceSample {
        source_center_x:sx,source_center_y:sy,
        source_left:left,source_top:top,
        source_width,source_height,
        valid_left,valid_top,valid_right,valid_bottom,
        lens_diameter_px,zoom,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    fn source()->SourceFrame {
        SourceFrame{width:1440,height:2560,rotation:0}
    }
    fn viewport()->ScreenViewport {
        ScreenViewport{x:100,y:200,width:360,height:640}
    }
    #[test]
    fn decoded_source_keeps_detail_lost_by_small_sdl_window() {
        let result=map_lens_to_decoded_frame(source(),viewport(),
            (280,520),200,2.35).unwrap();
        assert!((result.source_center_x-720.0).abs()<0.01);
        assert!((result.source_center_y-1280.0).abs()<0.01);
        // The decoded source is 4× denser than the 360×640 viewport.
        assert!((result.source_width-(200.0/2.35*4.0)).abs()<0.01);
        assert!((result.source_height-(200.0/2.35*4.0)).abs()<0.01);
        assert!(result.source_width>200.0);
    }
    #[test]
    fn lens_near_right_bottom_edge_preserves_source_focus() {
        let result=map_lens_to_decoded_frame(source(),viewport(),
            (459,839),200,2.35).unwrap();
        assert!(result.source_center_x>1430.0);
        assert!(result.source_center_y>2550.0);
        assert!(result.source_left+result.source_width>1440.0);
        assert!(result.source_top+result.source_height>2560.0);
        assert_eq!(result.valid_right,1440.0);
        assert_eq!(result.valid_bottom,2560.0);
        assert!(result.valid_left>=0.0);
    }
    #[test]
    fn landscape_rotation_maps_pointer_back_to_original_frame() {
        let landscape_view=ScreenViewport{x:-400,y:15,width:640,height:360};
        let rotated=SourceFrame{rotation:90,..source()};
        let result=map_lens_to_decoded_frame(rotated,landscape_view,
            (-240,195),200,2.35).unwrap(); // normalized 0.25,0.5
        assert!((result.source_center_x-720.0).abs()<0.01);
        assert!((result.source_center_y-1920.0).abs()<0.01);
        assert!(result.source_width>0.0 && result.source_height>0.0);
    }
    #[test]
    fn reverse_rotations_map_center_correctly() {
        for angle in [0,90,180,270] {
            let frame=SourceFrame{rotation:angle,..source()};
            let result=map_lens_to_decoded_frame(frame,viewport(),
                (280,520),220,2.35).unwrap();
            assert!((result.source_center_x-720.0).abs()<0.01);
            assert!((result.source_center_y-1280.0).abs()<0.01);
        }
    }
    #[test]
    fn rejects_bad_zoom_geometry_and_cross_device_outside_pointer() {
        for (cursor,zoom) in [((279,520),f32::NAN),
                              ((280,520),0.5),((280,520),4.0),
                              ((99,520),2.35),((280,840),2.35)] {
            assert!(map_lens_to_decoded_frame(source(),viewport(),
                    cursor,200,zoom).is_err());
        }
        assert!(map_lens_to_decoded_frame(
            SourceFrame{width:3900,..source()},viewport(),(280,520),200,2.35
        ).is_err());
        assert!(map_lens_to_decoded_frame(
            SourceFrame{rotation:15,..source()},viewport(),(280,520),200,2.35
        ).is_err());
    }
}
