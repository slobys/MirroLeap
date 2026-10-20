//! Windows owner/reader for the opt-in MirroLeap HQ decoded-frame bridge.
//! The normal MirroLeap renderer NEVER creates a mapping until an actual
//! patched scrcpy client/consumer is explicitly selected.
//!
//! Security: a BCrypt-generated per-session 128-bit random token names the
//! Local user mapping; only the child process receives it via its environment.
//! Tokens, pixels, frame data and mapping names are never logged.

#![cfg(target_os="windows")]

use std::{
    ffi::c_void,
    ptr::{self, NonNull},
    sync::atomic::{AtomicU32, Ordering},
};
use crate::hq_protocol::{
    self, FrameMetadata, OwnedFrame, HEADER_BYTES, MAPPING_BYTES, NO_FRAME,
    SLOT_CAPACITY, SLOT_COUNT, SLOT_HEADER_BYTES, VERSION,
};

const PAGE_READWRITE: u32=0x04;
const FILE_MAP_ALL_ACCESS: u32=0x000f_001f;
const ERROR_ALREADY_EXISTS: u32=183;
const BCRYPT_USE_SYSTEM_PREFERRED_RNG: u32=2;

#[link(name="kernel32")]
unsafe extern "system" {
    fn CreateFileMappingW(file:*mut c_void,attributes:*mut c_void,
        protection:u32,maximum_high:u32,maximum_low:u32,name:*const u16)->*mut c_void;
    fn MapViewOfFile(mapping:*mut c_void,access:u32,offset_high:u32,
        offset_low:u32,size:usize)->*mut c_void;
    fn UnmapViewOfFile(memory:*const c_void)->i32;
    fn CloseHandle(handle:*mut c_void)->i32;
    fn GetLastError()->u32;
}
#[link(name="bcrypt")]
unsafe extern "system" {
    fn BCryptGenRandom(algorithm:*mut c_void,buf:*mut u8,length:u32,flags:u32)->i32;
}
fn cryptographic_token()->Result<String,&'static str> {
    let mut bytes=[0u8;16];
    if unsafe {
        BCryptGenRandom(ptr::null_mut(),bytes.as_mut_ptr(),bytes.len() as u32,
                        BCRYPT_USE_SYSTEM_PREFERRED_RNG)
    } !=0 {
        return Err("Windows crypto RNG unavailable");
    }
    let mut token=String::with_capacity(32);
    for byte in bytes {
        use std::fmt::Write;
        write!(&mut token,"{byte:02x}").map_err(|_|"could not format HQ token")?;
    }
    Ok(token)
}

pub struct HqMappingOwner {
    mapping:NonNull<c_void>,
    base:NonNull<u8>,
    token:String,
}
impl HqMappingOwner {
    pub fn new()->Result<Self,&'static str> {
        for _ in 0..3 {
            let token=cryptographic_token()?;
            let name=format!("Local\\MirroLeapHQ_{token}");
            let wide:Vec<u16>=name.encode_utf16().chain(Some(0)).collect();
            let handle=unsafe {
                CreateFileMappingW((-1isize) as *mut c_void,ptr::null_mut(),
                                   PAGE_READWRITE,0,MAPPING_BYTES as u32,wide.as_ptr())
            };
            let Some(mapping)=NonNull::new(handle) else {
                return Err("Windows could not create shared HQ video memory");
            };
            if unsafe {GetLastError()}==ERROR_ALREADY_EXISTS {
                unsafe {CloseHandle(mapping.as_ptr());}
                continue; // never attach to a mapping created by someone else
            }
            let address=unsafe {
                MapViewOfFile(mapping.as_ptr(),FILE_MAP_ALL_ACCESS,0,0,MAPPING_BYTES)
            };
            let Some(base)=NonNull::new(address as *mut u8) else {
                unsafe {CloseHandle(mapping.as_ptr());}
                return Err("Windows could not map shared HQ video memory");
            };
            let owner=Self{mapping,base,token};
            owner.initialize();
            return Ok(owner);
        }
        Err("Windows could not reserve an isolated HQ frame session")
    }
    fn initialize(&self) {
        unsafe {
            ptr::write_bytes(self.base.as_ptr(),0,MAPPING_BYTES);
            ptr::copy_nonoverlapping(b"MLHQ".as_ptr(),self.base.as_ptr(),4);
            self.store_header(4,VERSION);
            self.store_header(8,MAPPING_BYTES as u32);
            self.store_header(12,SLOT_CAPACITY as u32);
            self.store_header(16,SLOT_COUNT as u32);
            self.store_header(20,NO_FRAME);
        }
    }
    unsafe fn store_header(&self,offset:usize,value:u32) {
        ptr::copy_nonoverlapping(value.to_le_bytes().as_ptr(),
                                 self.base.as_ptr().add(offset),4);
    }
    /// Pass via ONLY the explicitly patched child process's environment.
    /// Do not expose the mapping token to the UI, logs or telemetry.
    pub fn child_env_token(&self)->&str {&self.token}

    fn atomic_u32(&self,offset:usize)->u32 {
        unsafe {
            (*(self.base.as_ptr().add(offset) as *const AtomicU32))
                .load(Ordering::Acquire)
        }
    }

    /// Returns None while the producer is busy, no frame is available,
    /// or a slot was concurrently replaced; the UI should keep its previous
    /// frame rather than waiting on the producer.
    pub fn try_latest(&self)->Result<Option<OwnedFrame>,&'static str> {
        let global=unsafe {
            std::slice::from_raw_parts(self.base.as_ptr(),MAPPING_BYTES)
        };
        hq_protocol::validate_header(global)?;
        let slot=self.atomic_u32(20);
        if slot==NO_FRAME {return Ok(None);}
        if slot>=SLOT_COUNT as u32 {return Err("HQ slot index out of bounds");}
        let slot_start=HEADER_BYTES + slot as usize * (SLOT_HEADER_BYTES+SLOT_CAPACITY);
        let first=self.atomic_u32(slot_start);
        if first==0 || first&1!=0 {return Ok(None);}
        let mut header=[0u8;SLOT_HEADER_BYTES];
        unsafe {
            ptr::copy_nonoverlapping(self.base.as_ptr().add(slot_start),
                                     header.as_mut_ptr(),header.len());
        }
        let metadata:FrameMetadata=hq_protocol::validate_frame_header(&header)?;
        if metadata.sequence!=first {return Ok(None);}
        let mut data=vec![0u8;metadata.bytes];
        unsafe {
            ptr::copy_nonoverlapping(
                self.base.as_ptr().add(slot_start+SLOT_HEADER_BYTES),
                data.as_mut_ptr(),metadata.bytes,
            );
        }
        let second=self.atomic_u32(slot_start);
        if first!=second || second&1!=0 {return Ok(None);}
        Ok(Some(OwnedFrame{metadata,data}))
    }
}
impl Drop for HqMappingOwner {
    fn drop(&mut self) {
        unsafe {
            UnmapViewOfFile(self.base.as_ptr() as *const c_void);
            CloseHandle(self.mapping.as_ptr());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn owner_creates_private_memory_and_initially_has_no_frame() {
        let owner=HqMappingOwner::new().expect("Windows HQ mapping");
        assert_eq!(owner.child_env_token().len(),32);
        assert!(owner.child_env_token().bytes()
                .all(|b|b.is_ascii_digit() || (b'a'..=b'f').contains(&b)));
        assert_eq!(owner.try_latest().unwrap(),None);
    }
    #[test]
    fn distinct_hq_sessions_are_isolated() {
        let a=HqMappingOwner::new().expect("mapping A");
        let b=HqMappingOwner::new().expect("mapping B");
        assert_ne!(a.child_env_token(),b.child_env_token());
        assert!(a.try_latest().unwrap().is_none());
        assert!(b.try_latest().unwrap().is_none());
    }
    #[test]
    fn reader_never_exposes_uncommitted_or_malformed_frames() {
        let owner=HqMappingOwner::new().expect("HQ map");
        let s=HEADER_BYTES;
        unsafe {
            let base=owner.base.as_ptr();
            ptr::copy_nonoverlapping(0u32.to_le_bytes().as_ptr(),
                                     base.add(s),4);
            ptr::copy_nonoverlapping(0u32.to_le_bytes().as_ptr(),
                                     base.add(20),4);
        }
        assert!(owner.try_latest().unwrap().is_none());
        unsafe {
            ptr::copy_nonoverlapping(1u32.to_le_bytes().as_ptr(),
                                     owner.base.as_ptr().add(s),4);
        }
        assert!(owner.try_latest().unwrap().is_none());
    }
}
