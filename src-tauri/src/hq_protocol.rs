//! Experimental decoded-frame transport, byte-compatible with
//! hq/include/mirroleap_hq_protocol.h. This module does NOT claim that a
//! high-definition GPU renderer already exists.
//!
//! It is deliberately independent of Windows, so malformed buffers, frame
//! sizes, rotation and source formats can be regression-tested on Linux CI.

pub const VERSION: u32 = 1;
pub const HEADER_BYTES: usize = 64;
pub const SLOT_HEADER_BYTES: usize = 80;
pub const SLOT_COUNT: usize = 2;
pub const MAX_WIDTH: u32 = 3840;
pub const MAX_HEIGHT: u32 = 3840;
pub const MAX_PIXELS: usize = 3840 * 2160;
pub const SLOT_CAPACITY: usize = MAX_PIXELS * 3 / 2;
pub const MAPPING_BYTES: usize =
    HEADER_BYTES + SLOT_COUNT * (SLOT_HEADER_BYTES + SLOT_CAPACITY);
pub const NO_FRAME: u32 = u32::MAX;
const MAGIC: &[u8;4] = b"MLHQ";

#[derive(Clone,Copy,Debug,PartialEq,Eq)]
#[repr(u32)]
pub enum PixelFormat { Yuv420p=1, Nv12=2 }

#[derive(Clone,Debug,PartialEq,Eq)]
pub struct FrameMetadata {
    pub format: PixelFormat,
    pub width: u32,
    pub height: u32,
    pub pts: i64,
    pub sequence: u32,
    pub strides: [u32;3],
    pub offsets: [u32;3],
    pub lengths: [u32;3],
    pub bytes: usize,
    pub rotation_degrees: u32,
}

#[derive(Clone,Debug,PartialEq,Eq)]
pub struct OwnedFrame {
    pub metadata: FrameMetadata,
    pub data: Vec<u8>,
}

fn read_u32(data:&[u8],offset:usize)->Result<u32,&'static str> {
    let bytes=data.get(offset..offset+4).ok_or("truncated HQ u32")?;
    Ok(u32::from_le_bytes(bytes.try_into().unwrap()))
}
fn read_i64(data:&[u8],offset:usize)->Result<i64,&'static str> {
    let bytes=data.get(offset..offset+8).ok_or("truncated HQ i64")?;
    Ok(i64::from_le_bytes(bytes.try_into().unwrap()))
}

/// Validate the immutable 64-byte global prefix. The live reader must
/// not construct an immutable Rust slice over the entire memory mapping:
/// the C producer concurrently writes the frame-slot region.
pub fn validate_header_prefix(data:&[u8])->Result<(),&'static str> {
    if data.len()!=HEADER_BYTES {return Err("wrong HQ header size");}
    if data.get(..4)!=Some(MAGIC.as_slice()) {return Err("HQ magic mismatch");}
    if read_u32(data,4)?!=VERSION {return Err("HQ protocol version unsupported");}
    if read_u32(data,8)? as usize != MAPPING_BYTES
        || read_u32(data,12)? as usize != SLOT_CAPACITY
        || read_u32(data,16)? as usize != SLOT_COUNT {
        return Err("HQ mapping geometry mismatch");
    }
    Ok(())
}

pub fn validate_header(data:&[u8])->Result<(),&'static str> {
    if data.len()!=MAPPING_BYTES {return Err("wrong HQ mapping size");}
    validate_header_prefix(&data[..HEADER_BYTES])
}

fn slot_start(index:usize)->Result<usize,&'static str> {
    if index>=SLOT_COUNT {return Err("invalid HQ slot index");}
    Ok(HEADER_BYTES+index*(SLOT_HEADER_BYTES+SLOT_CAPACITY))
}

pub fn validate_frame_header(slot:&[u8])->Result<FrameMetadata,&'static str> {
    if slot.len()<SLOT_HEADER_BYTES {return Err("truncated HQ slot header");}
    let seq=read_u32(slot,0)?;
    if seq==0 || seq&1!=0 {return Err("HQ frame is not committed");}
    let format=match read_u32(slot,4)? {
        1=>PixelFormat::Yuv420p,
        2=>PixelFormat::Nv12,
        _=>return Err("unsupported HQ pixel format"),
    };
    let width=read_u32(slot,8)?;
    let height=read_u32(slot,12)?;
    if width<2 || height<2 || width>MAX_WIDTH || height>MAX_HEIGHT
        || width&1!=0 || height&1!=0
        || (width as u64)*(height as u64)>MAX_PIXELS as u64 {
        return Err("unsupported HQ decoded frame dimensions");
    }
    let y=(width as usize).checked_mul(height as usize)
        .ok_or("HQ frame pixel count overflow")?;
    let half=y/4;
    let expected_bytes=y.checked_add(y/2).ok_or("HQ plane size overflow")?;
    if expected_bytes>SLOT_CAPACITY {return Err("HQ plane payload exceeds capacity");}
    let format_layout=match format {
        PixelFormat::Yuv420p=>(
            [width,width/2,width/2],
            [0,y as u32,(y+half) as u32],
            [y as u32,half as u32,half as u32]),
        PixelFormat::Nv12=>(
            [width,width,0],
            [0,y as u32,0],
            [y as u32,(y/2) as u32,0]),
    };
    let strides=[read_u32(slot,24)?,read_u32(slot,28)?,read_u32(slot,32)?];
    let offsets=[read_u32(slot,36)?,read_u32(slot,40)?,read_u32(slot,44)?];
    let lengths=[read_u32(slot,48)?,read_u32(slot,52)?,read_u32(slot,56)?];
    if (strides,offsets,lengths)!=format_layout {
        return Err("HQ frame planes must be packed and canonical");
    }
    if read_u32(slot,60)? as usize!=expected_bytes {
        return Err("HQ frame payload size mismatch");
    }
    let rotation=read_u32(slot,64)?;
    if !matches!(rotation,0|90|180|270) {
        return Err("unsupported HQ rotation");
    }
    Ok(FrameMetadata {
        format,width,height,pts:read_i64(slot,16)?,sequence:seq,
        strides,offsets,lengths,bytes:expected_bytes,
        rotation_degrees:rotation,
    })
}

/// Pure in-memory test/probe reader. Reading a *concurrently written* Windows
/// mapping additionally requires the before/after atomic slot sequence check.
pub fn read_latest_snapshot(data:&[u8])->Result<Option<OwnedFrame>,&'static str> {
    validate_header(data)?;
    let latest=read_u32(data,20)?;
    if latest==NO_FRAME {return Ok(None);}
    let start=slot_start(latest as usize)?;
    let metadata=validate_frame_header(&data[start..start+SLOT_HEADER_BYTES])?;
    let payload_start=start+SLOT_HEADER_BYTES;
    let payload_end=payload_start+metadata.bytes;
    let payload=data.get(payload_start..payload_end)
        .ok_or("truncated HQ decoded frame")?;
    Ok(Some(OwnedFrame{metadata,data:payload.to_vec()}))
}

#[cfg(test)]
mod tests {
    use super::*;
    fn set32(buf:&mut [u8],at:usize,value:u32) {
        buf[at..at+4].copy_from_slice(&value.to_le_bytes());
    }
    fn build(format:PixelFormat,width:u32,height:u32)->Vec<u8> {
        let mut buf=vec![0u8;MAPPING_BYTES];
        buf[..4].copy_from_slice(MAGIC);
        set32(&mut buf,4,VERSION);
        set32(&mut buf,8,MAPPING_BYTES as u32);
        set32(&mut buf,12,SLOT_CAPACITY as u32);
        set32(&mut buf,16,SLOT_COUNT as u32);
        set32(&mut buf,20,0);
        let h=slot_start(0).unwrap();
        set32(&mut buf,h,2);
        set32(&mut buf,h+4,format as u32);
        set32(&mut buf,h+8,width);
        set32(&mut buf,h+12,height);
        let y=width*height;
        set32(&mut buf,h+24,width);
        set32(&mut buf,h+36,0);
        set32(&mut buf,h+48,y);
        if format==PixelFormat::Yuv420p {
            set32(&mut buf,h+28,width/2);
            set32(&mut buf,h+32,width/2);
            set32(&mut buf,h+36,0);
            set32(&mut buf,h+40,y);
            set32(&mut buf,h+44,y+y/4);
            set32(&mut buf,h+52,y/4);
            set32(&mut buf,h+56,y/4);
        }else{
            set32(&mut buf,h+28,width);
            set32(&mut buf,h+40,y);
            set32(&mut buf,h+52,y/2);
        }
        set32(&mut buf,h+60,y*3/2);
        buf
    }
    #[test]
    fn protocol_header_checks_magic_version_and_capacity() {
        let mut b=build(PixelFormat::Nv12,4,4);
        assert!(validate_header(&b).is_ok());
        b[0]=b'X';
        assert!(validate_header(&b).is_err());
        b[0]=b'M';set32(&mut b,4,2);
        assert!(validate_header(&b).is_err());
        set32(&mut b,4,1);set32(&mut b,12,1);
        assert!(validate_header(&b).is_err());
    }
    #[test]
    fn global_prefix_can_be_checked_without_aliasing_a_mutating_frame_slot() {
        let data=build(PixelFormat::Nv12,4,4);
        assert!(validate_header_prefix(&data[..HEADER_BYTES]).is_ok());
        assert!(validate_header_prefix(&data[..HEADER_BYTES-1]).is_err());
        assert!(validate_header_prefix(&data[..HEADER_BYTES+1]).is_err());
    }
    #[test]
    fn last_published_slot_has_validated_packed_420_layout() {
        let yuv=build(PixelFormat::Yuv420p,4,4);
        let frame=read_latest_snapshot(&yuv).unwrap().unwrap();
        assert_eq!(frame.metadata.strides,[4,2,2]);
        assert_eq!(frame.metadata.offsets,[0,16,20]);
        assert_eq!(frame.metadata.lengths,[16,4,4]);
        assert_eq!(frame.data.len(),24);
        let nv12=build(PixelFormat::Nv12,4,4);
        let f=read_latest_snapshot(&nv12).unwrap().unwrap();
        assert_eq!(f.metadata.offsets,[0,16,0]);
        assert_eq!(f.metadata.lengths,[16,8,0]);
    }
    #[test]
    fn ignores_no_frame_and_rejects_in_progress_slots() {
        let mut b=build(PixelFormat::Nv12,4,4);
        set32(&mut b,20,NO_FRAME);
        assert_eq!(read_latest_snapshot(&b).unwrap(),None);
        set32(&mut b,20,0);
        set32(&mut b,slot_start(0).unwrap(),3);
        assert!(read_latest_snapshot(&b).is_err());
        set32(&mut b,slot_start(0).unwrap(),0);
        assert!(read_latest_snapshot(&b).is_err());
    }
    #[test]
    fn rejects_out_of_range_dimensions_formats_and_sequence() {
        let mut b=build(PixelFormat::Yuv420p,4,4);
        let h=slot_start(0).unwrap();
        for (at,bad) in [(h+4,99),(h+8,MAX_WIDTH+2),(h+12,0),
                         (h+8,3),(h+60,12345),(h+64,45)] {
            let saved=read_u32(&b,at).unwrap();
            set32(&mut b,at,bad);
            assert!(read_latest_snapshot(&b).is_err(),"{at}");
            set32(&mut b,at,saved);
        }
        assert!(read_latest_snapshot(&b).is_ok());
    }
    #[test]
    fn no_payload_trust_if_offets_are_malformed() {
        let mut b=build(PixelFormat::Nv12,1920,1080);
        let h=slot_start(0).unwrap();
        set32(&mut b,h+40,u32::MAX);
        assert!(read_latest_snapshot(&b).is_err());
    }
    #[test]
    fn parses_the_actual_c_producer_fixture_byte_for_byte() {
        let Ok(path)=std::env::var("MIRROLEAP_HQ_C_FIXTURE") else {return;};
        let bytes=std::fs::read(path).expect("C producer fixture");
        let frame=read_latest_snapshot(&bytes).expect("frame protocol")
            .expect("latest C frame");
        assert_eq!(frame.metadata.format,PixelFormat::Nv12);
        assert_eq!((frame.metadata.width,frame.metadata.height),(4,4));
        assert_eq!(frame.metadata.pts,1_234_567);
        assert_eq!(frame.metadata.strides,[4,4,0]);
        assert_eq!(frame.metadata.offsets,[0,16,0]);
        assert_eq!(frame.data.len(),24);
        let expected_y:Vec<u8>=(1u8..=16).collect();
        let expected_uv:Vec<u8>=(220u8..=227).collect();
        assert_eq!(&frame.data[..16],expected_y.as_slice());
        assert_eq!(&frame.data[16..],expected_uv.as_slice());
    }

    #[test]
    fn tall_phone_frames_are_accepted_without_increasing_mapping_capacity() {
        let tall=build(PixelFormat::Nv12,1440,2560);
        assert!(read_latest_snapshot(&tall).is_ok());
        let four_k_portrait=build(PixelFormat::Yuv420p,2160,3840);
        assert!(read_latest_snapshot(&four_k_portrait).is_ok());
        let too_large=build(PixelFormat::Nv12,3840,3840);
        assert!(read_latest_snapshot(&too_large).is_err());
    }

    #[test]
    fn mapping_geometry_stays_below_safe_32bit_cap() {
        assert!(MAPPING_BYTES < u32::MAX as usize);
        assert_eq!(SLOT_CAPACITY,12_441_600);
        assert_eq!(MAPPING_BYTES,24_883_424);
    }
}
