//! Cross-platform CPU **reference** crop of the decoded MirroCreator HQ frame.
//! It establishes exact source-space 2.35× sampling semantics before a
//! Windows D3D11 implementation is allowed to replace this test oracle.
//!
//! This is NOT enabled as a live renderer and must not run on the scrcpy
//! decoder thread. Expensive frame work belongs behind a bounded queue.
//! Default output is premultiplied-independent BGRA (alpha 255). The color
//! transform is BT.709 limited-range as a *reference approximation*;
//! metadata for actual scrcpy stream range/matrix is a required next step.

use crate::hq_geometry::{SourceFrame, SourceSample};
use crate::hq_protocol::{OwnedFrame, PixelFormat};

pub const MAX_REFERENCE_DIAMETER: u32 = 720;
const BGRA_BACKGROUND: [u8;4] = [23,14,8,255];

fn bilinear_plane(
    data:&[u8], offset:usize, stride:usize, width:usize, height:usize,
    pixel_x:f32,pixel_y:f32, step:usize, component:usize,
)->Result<f32,&'static str> {
    if width==0 || height==0 || stride==0 || step==0 {
        return Err("invalid HQ plane geometry");
    }
    let x=pixel_x.clamp(0.0,(width-1) as f32);
    let y=pixel_y.clamp(0.0,(height-1) as f32);
    let x0=x.floor() as usize;
    let y0=y.floor() as usize;
    let x1=x0.saturating_add(1).min(width-1);
    let y1=y0.saturating_add(1).min(height-1);
    let a=x-x0 as f32;
    let b=y-y0 as f32;
    let read=|x:usize,y:usize|->Result<f32,&'static str> {
        let at=offset.checked_add(
            y.checked_mul(stride).ok_or("HQ plane row offset overflow")?,
        ).and_then(|n|n.checked_add(x.checked_mul(step)?))
            .and_then(|n|n.checked_add(component))
            .ok_or("HQ plane pixel offset overflow")?;
        Ok(*data.get(at).ok_or("HQ image payload truncated")? as f32)
    };
    let t0=read(x0,y0)?*(1.0-a)+read(x1,y0)?*a;
    let t1=read(x0,y1)?*(1.0-a)+read(x1,y1)?*a;
    Ok(t0*(1.0-b)+t1*b)
}

fn sample_rgb(frame:&OwnedFrame, x:f32,y:f32)
    ->Result<[u8;4],&'static str>
{
    let w=frame.metadata.width as usize;
    let h=frame.metadata.height as usize;
    if x< -0.5 || y< -0.5 || x> w as f32 - 0.5 || y> h as f32 - 0.5 {
        return Ok(BGRA_BACKGROUND);
    }
    let data=&frame.data;
    let y_plane=bilinear_plane(data,0,w,w,h,x,y,1,0)?;
    let (u,v)=match frame.metadata.format {
        PixelFormat::Yuv420p=>{
            let y_len=w.checked_mul(h).ok_or("HQ Y plane overflow")?;
            let uv_len=y_len/4;
            let cx=x/2.0;
            let cy=y/2.0;
            let u=bilinear_plane(data,y_len,w/2,w/2,h/2,cx,cy,1,0)?;
            let v=bilinear_plane(data,y_len+uv_len,w/2,w/2,h/2,cx,cy,1,0)?;
            (u,v)
        },
        PixelFormat::Nv12=>{
            let y_len=w.checked_mul(h).ok_or("HQ Y plane overflow")?;
            let cx=x/2.0;
            let cy=y/2.0;
            let u=bilinear_plane(data,y_len,w,w/2,h/2,cx,cy,2,0)?;
            let v=bilinear_plane(data,y_len,w,w/2,h/2,cx,cy,2,1)?;
            (u,v)
        },
    };
    // BT.709 limited range; actual pixel metadata must override before any
    // user-visible GPU renderer enables color-critical output.
    let yy=(y_plane-16.0)*1.164_383_5;
    let uu=u-128.0;
    let vv=v-128.0;
    let r=(yy+1.792_741*vv).clamp(0.0,255.0);
    let g=(yy-0.213_249*uu-0.532_909*vv).clamp(0.0,255.0);
    let b=(yy+2.112_402*uu).clamp(0.0,255.0);
    Ok([b.round() as u8,g.round() as u8,r.round() as u8,255])
}

/// Renders a fixed lens of diameter `out_size` from raw decoded AVFrame
/// pixels, NOT from already-downscaled SDL/window pixels. This is a bounded
/// parity oracle for future Windows GPU rendering, not a production loop.
pub fn render_decoded_reference(
    frame:&OwnedFrame, source:SourceFrame, area:&SourceSample,
    out_size:u32,
)->Result<Vec<u8>,&'static str> {
    if out_size<1 || out_size>MAX_REFERENCE_DIAMETER {
        return Err("HQ reference lens diameter outside bounds");
    }
    if area.lens_diameter_px!=out_size {
        return Err("HQ reference output diameter must match source crop");
    }
    if source.width!=frame.metadata.width || source.height!=frame.metadata.height
        || source.rotation!=frame.metadata.rotation_degrees {
        return Err("HQ reference source/metadata mismatch");
    }
    if frame.data.len()!=frame.metadata.bytes {
        return Err("HQ reference payload length mismatch");
    }
    if !matches!(source.rotation,0|90|180|270) ||
        !area.source_width.is_finite() || !area.source_height.is_finite()
        || area.source_width<=0.0 || area.source_height<=0.0
        || !area.source_left.is_finite() || !area.source_top.is_finite()
        || !area.zoom.is_finite() || area.zoom<1.0 || area.zoom>3.5 {
        return Err("invalid HQ reference crop");
    }
    let size=out_size as usize;
    let mut out=vec![0u8;size*size*4];
    for dy in 0..size {
        let v=(dy as f32+0.5)/out_size as f32;
        for dx in 0..size {
            let u=(dx as f32+0.5)/out_size as f32;
            let (cx,cy)=match source.rotation {
                0=>(u,v),
                90=>(v,1.0-u),
                180=>(1.0-u,1.0-v),
                270=>(1.0-v,u),
                _=>unreachable!(),
            };
            // Decoded image pixels are sampled before any SDL downscale.
            // Subtract half a pixel to align interpolated sample centers.
            let x=area.source_left+cx*area.source_width-0.5;
            let y=area.source_top+cy*area.source_height-0.5;
            let bgra=sample_rgb(frame,x,y)?;
            out[(dy*size+dx)*4..(dy*size+dx+1)*4].copy_from_slice(&bgra);
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hq_geometry::{map_lens_to_decoded_frame,ScreenViewport};
    use crate::hq_protocol::FrameMetadata;

    fn sample_frame(format:PixelFormat, width:u32,height:u32)->OwnedFrame {
        let y_len=(width*height) as usize;
        let bytes=y_len*3/2;
        let (strides,offsets,lengths)=match format {
            PixelFormat::Yuv420p=>([width,width/2,width/2],
                [0,y_len as u32,(y_len+y_len/4) as u32],
                [y_len as u32,(y_len/4) as u32,(y_len/4) as u32]),
            PixelFormat::Nv12=>([width,width,0],
                [0,y_len as u32,0],
                [y_len as u32,(y_len/2) as u32,0]),
        };
        let mut data=vec![128u8;bytes];
        data[..y_len].fill(16); // black limited-range luma
        OwnedFrame{metadata:FrameMetadata{
            format,width,height,pts:123,sequence:2,strides,offsets,
            lengths,bytes,rotation_degrees:0,
        },data}
    }
    fn crop(frame:&OwnedFrame,lens:u32)->SourceSample {
        map_lens_to_decoded_frame(SourceFrame{
            width:frame.metadata.width,height:frame.metadata.height,rotation:0,
        },ScreenViewport{x:0,y:0,width:128,
            height:128},(64,64),
            lens,2.35).unwrap()
    }
    #[test]
    fn both_native_formats_produce_consistent_bgra_neutral_video() {
        for format in [PixelFormat::Yuv420p,PixelFormat::Nv12] {
            let f=sample_frame(format,8,8);
            let img=render_decoded_reference(&f,SourceFrame{
                width:8,height:8,rotation:0,
            },&crop(&f,40),40).unwrap();
            assert_eq!(img.len(),40*40*4);
            for pixel in img.chunks_exact(4) {
                assert_eq!(pixel,&[0,0,0,255]);
            }
        }
    }
    #[test]
    fn preserve_source_pixel_detail_without_desktop_downsampling() {
        let mut f=sample_frame(PixelFormat::Nv12,16,16);
        // Two adjacent 1-source-pixel white marks would be averaged or lost
        // if the 16px video was first downscaled into a 4px SDL window.
        f.data[8*16+7]=235;
        f.data[8*16+8]=235;
        let sample=crop(&f,40);
        let img=render_decoded_reference(&f,SourceFrame{
            width:16,height:16,rotation:0,
        },&sample,40).unwrap();
        assert!(img.chunks_exact(4).any(|p|p[2]>=180),
                "expected original bright center detail to remain visible");
        assert!(img.chunks_exact(4).any(|p|p[2]<30),
                "must also retain nearby black pixel detail");
    }
    #[test]
    fn edges_are_filled_without_repeating_clipped_app_pixels() {
        let f=sample_frame(PixelFormat::Yuv420p,16,16);
        let mut area=crop(&f,40);
        area.source_left=-2.0;
        area.source_top=-2.0;
        let img=render_decoded_reference(&f,SourceFrame{
            width:16,height:16,rotation:0,
        },&area,40).unwrap();
        assert_eq!(&img[..4],&BGRA_BACKGROUND);
        assert_eq!(&img[(40*40-1)*4..(40*40)*4],&[0,0,0,255]);
    }
    #[test]
    fn reject_truncated_and_mismatched_or_oversize_frames() {
        let mut f=sample_frame(PixelFormat::Nv12,8,8);
        let sample=crop(&f,40);
        f.data.pop();
        assert!(render_decoded_reference(&f,SourceFrame{
            width:8,height:8,rotation:0,
        },&sample,40).is_err());
        f.data.push(128);
        assert!(render_decoded_reference(&f,SourceFrame{
            width:8,height:8,rotation:90,
        },&sample,40).is_err());
        assert!(render_decoded_reference(&f,SourceFrame{
            width:8,height:8,rotation:0,
        },&sample,MAX_REFERENCE_DIAMETER+1).is_err());
    }
    #[test]
    fn direct_lens_samples_work_for_rotated_decoded_video() {
        let mut f=sample_frame(PixelFormat::Nv12,8,16);
        f.metadata.rotation_degrees=90;
        let source=SourceFrame{width:8,height:16,rotation:90};
        let area=map_lens_to_decoded_frame(source,
            ScreenViewport{x:-100,y:50,width:160,height:80},
            (-20,90),64,2.35).unwrap();
        let rgba=render_decoded_reference(&f,source,&area,64).unwrap();
        assert_eq!(rgba.len(),64*64*4);
        assert!(rgba.chunks_exact(4).any(|p|p==[0,0,0,255]));
    }
}
