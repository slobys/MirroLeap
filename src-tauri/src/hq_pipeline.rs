//! Stage-2 MirroCreator HQ image path: already-decoded AVFrame memory
//! -> validated source-space crop -> bounded BGRA preview pixels.
//!
//! This connects the tested transport and 2.35× source sampling reference.
//! It deliberately does NOT create a second scrcpy stream or expose a new
//! runtime UI option. Rendering on a production GPU swapchain is stage 3.

use crate::hq_geometry::{map_lens_to_decoded_frame,ScreenViewport,SourceFrame};
use crate::hq_protocol::{OwnedFrame, PixelFormat};
use crate::hq_reference_render::render_decoded_reference;

pub struct PreviewPixels {
    pub width:u32,
    pub height:u32,
    pub pts:i64,
    pub sequence:u32,
    pub format:PixelFormat,
    pub bgra:Vec<u8>,
}

pub fn render_decoded_hq_preview(
    frame:&OwnedFrame,
    viewport:ScreenViewport,
    cursor:(i32,i32),
    lens_diameter:u32,
    zoom:f32,
)->Result<PreviewPixels,&'static str> {
    if lens_diameter<40 || lens_diameter>720 {
        return Err("HQ lens diameter out of bounds");
    }
    let src=SourceFrame {
        width:frame.metadata.width,height:frame.metadata.height,
        rotation:frame.metadata.rotation_degrees,
    };
    let rect=map_lens_to_decoded_frame(src,viewport,cursor,lens_diameter,zoom)?;
    let bgra=render_decoded_reference(frame,src,&rect,lens_diameter)?;
    Ok(PreviewPixels {
        width:lens_diameter,height:lens_diameter,
        pts:frame.metadata.pts,sequence:frame.metadata.sequence,
        format:frame.metadata.format,bgra,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hq_protocol::{self,FrameMetadata};

    fn fixture_frame()->OwnedFrame {
        let width=1440u32;
        let height=2560u32;
        let luma=(width*height) as usize;
        let mut data=vec![128u8;luma*3/2];
        data[..luma].fill(16);
        // A tiny 4x4 bright feature is present in the original 1440x2560
        // frame but could shrink to a single pixel once SDL scales to 360x640.
        for y in 1278..=1281usize {
            for x in 718..=721usize {
                data[y*width as usize+x]=235;
            }
        }
        OwnedFrame {
            metadata:FrameMetadata {
                width,height,format:PixelFormat::Nv12,pts:99_999,sequence:2,
                strides:[width,width,0],
                offsets:[0,luma as u32,0],
                lengths:[luma as u32,(luma/2) as u32,0],
                bytes:luma*3/2,rotation_degrees:0,
            },
            data,
        }
    }
    #[test]
    fn retains_small_real_phone_details_at_creator_preferred_235x() {
        let frame=fixture_frame();
        let viewport=ScreenViewport{x:100,y:200,width:360,height:640};
        let result=render_decoded_hq_preview(
            &frame,viewport,(280,520),200,2.35).unwrap();
        assert_eq!((result.width,result.height),(200,200));
        assert_eq!(result.pts,99_999);
        assert_eq!(result.bgra.len(),200*200*4);
        assert!(result.bgra.chunks_exact(4).any(|p|p[2]>=180),
                "original high-frequency phone detail must remain visible");
        assert!(result.bgra.chunks_exact(4).any(|p|p[2]<30));
    }
    #[test]
    fn cannot_render_another_phone_or_unsafe_magnifier_size() {
        let frame=fixture_frame();
        let screen=ScreenViewport{x:-320,y:50,width:360,height:640};
        assert!(render_decoded_hq_preview(&frame,screen,(400,100),200,2.35).is_err());
        assert!(render_decoded_hq_preview(&frame,screen,(-140,300),721,2.35).is_err());
        assert!(render_decoded_hq_preview(&frame,screen,(-140,300),200,4.5).is_err());
        assert!(render_decoded_hq_preview(&frame,screen,(-140,300),200,f32::NAN).is_err());
    }
    #[test]
    fn synthetic_c_packed_nv12_mapping_flows_into_bgra_pixels() {
        let Ok(path)=std::env::var("MIRROLEAP_HQ_C_FIXTURE") else {return;};
        let contents=std::fs::read(path).expect("C producer output file");
        let frame=hq_protocol::read_latest_snapshot(&contents).unwrap().unwrap();
        let output=render_decoded_hq_preview(&frame,
            ScreenViewport{x:0,y:0,width:120,height:120},(60,60),40,2.35
        ).unwrap();
        assert_eq!(output.format,PixelFormat::Nv12);
        assert_eq!(output.pts,1234567);
        assert_eq!(output.bgra.len(),40*40*4);
        assert!(output.bgra.chunks_exact(4).all(|px|px[3]==255));
    }
}
