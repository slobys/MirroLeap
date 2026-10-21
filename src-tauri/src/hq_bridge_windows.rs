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
    sync::atomic::{AtomicU32, AtomicU64, Ordering},
};
use crate::hq_protocol::{
    self, FrameMetadata, OwnedFrame, HEADER_BYTES, MAPPING_BYTES, NO_FRAME,
    SLOT_CAPACITY, SLOT_COUNT, SLOT_HEADER_BYTES, VERSION, CONSUMER_ACTIVE_OFFSET,
    PRODUCER_HEALTH_OFFSET,
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
    generation:AtomicU64,
}
// Windows mapping handles are valid across threads. The bytes belong to an
// independent scrcpy process; consumers only use bounded atomic snapshots and
// never expose a mutable Rust slice to the shared mapping.
unsafe impl Send for HqMappingOwner {}
unsafe impl Sync for HqMappingOwner {}

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
            let owner=Self{mapping,base,token,generation:AtomicU64::new(0)};
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

    /// Set demand ONLY for a currently visible HQ circle. When disabled the
    /// patched scrcpy sink neither clones nor copies decoded pixels.
    pub fn set_consumer_active(&self,active:bool) {
        let flag=unsafe{&*(self.base.as_ptr().add(CONSUMER_ACTIVE_OFFSET) as *const AtomicU32)};
        if active {
            if flag.load(Ordering::Acquire)==0 {
                // Clear old source pixels before allowing a fresh producer.
                self.reset_publication();
                flag.store(1,Ordering::Release);
            }
        } else {
            flag.store(0,Ordering::Release);
        }
    }
    pub fn consumer_active(&self)->bool {
        self.atomic_u32(CONSUMER_ACTIVE_OFFSET)==1
    }

    /// Returns None while the producer is busy, no frame is available,
    /// or a slot was concurrently replaced; the UI should keep its previous
    /// frame rather than waiting on the producer.
    ///
    /// IMPORTANT: the slot sequence is local to each of two slots (both can
    /// initially be 2), so do NOT use it to determine whether the visible
    /// image is newer. The global publish ID at header offset 24 uniquely
    /// advances for each committed frame.
    pub fn try_latest_with_publish_id(
        &self,
    )->Result<Option<(u32,OwnedFrame)>,&'static str> {
        // Only copy the immutable 0..20 bytes of the shared header. Creating
        // &[u8] over the entire mapping while C updates the frame slots
        // would produce unsound read-only aliases of mutating shared pixels.
        let mut fixed=[0u8;HEADER_BYTES];
        unsafe {
            ptr::copy_nonoverlapping(self.base.as_ptr(),fixed.as_mut_ptr(),20);
        }
        hq_protocol::validate_header_prefix(&fixed)?;
        if self.atomic_u32(PRODUCER_HEALTH_OFFSET)!=0 {
            return Err("HQ GPU source disabled after slow hardware readback");
        }
        let publish_before=self.atomic_u32(24);
        let slot=self.atomic_u32(20);
        if slot==NO_FRAME {return Ok(None);}
        if slot>=SLOT_COUNT as u32 {return Err("HQ slot index out of bounds");}
        if publish_before==0 {return Ok(None);}
        let slot_start=HEADER_BYTES + slot as usize*(SLOT_HEADER_BYTES+SLOT_CAPACITY);
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
        let last_slot=self.atomic_u32(20);
        let publish_after=self.atomic_u32(24);
        if first!=second || second&1!=0 || last_slot!=slot ||
            publish_before!=publish_after {
            return Ok(None);
        }
        Ok(Some((publish_after,OwnedFrame{metadata,data})))
    }

    /// Cheap readiness probe: do not copy a 2K/4K frame every 16 ms when
    /// scrcpy has not published anything new and the cursor is stationary.
    pub fn latest_publish_id(&self)->Result<Option<u32>,&'static str> {
        let mut header=[0u8;HEADER_BYTES];
        unsafe {ptr::copy_nonoverlapping(self.base.as_ptr(),header.as_mut_ptr(),20);}
        hq_protocol::validate_header_prefix(&header)?;
        if self.atomic_u32(PRODUCER_HEALTH_OFFSET)!=0 {
            return Err("HQ GPU readback too slow; using desktop magnifier");
        }
        let sequence=self.atomic_u32(24);
        let slot=self.atomic_u32(20);
        if slot==NO_FRAME || sequence==0 {return Ok(None);}
        if slot>=SLOT_COUNT as u32 {return Err("HQ slot index out of bounds");}
        Ok(Some(sequence))
    }

    /// Called only AFTER the previous producer exited and before a fresh
    /// experimental process starts. UI toggling alone must NOT clear a
    /// slow-GPU flag because the same v5 worker will stay disabled.
    pub fn clear_producer_health_for_new_process(&self) {
        let health=unsafe {&*(self.base.as_ptr().add(PRODUCER_HEALTH_OFFSET)
            as *const AtomicU32)};
        health.store(0,Ordering::Release);
    }

    /// Distinguish a restarted producer's publish id=1 from the old id=1.
    pub fn generation(&self)->u64 {self.generation.load(Ordering::Acquire)}

    /// Call only after the previous patched scrcpy process fully exited,
    /// before a decoder/audio retry can publish into this same session.
    pub fn reset_publication(&self) {
        unsafe {
            (*(self.base.as_ptr().add(20) as *const AtomicU32))
                .store(NO_FRAME,Ordering::Release);
            (*(self.base.as_ptr().add(24) as *const AtomicU32))
                .store(0,Ordering::Release);
        }
        self.generation.fetch_add(1,Ordering::AcqRel);
    }

    pub fn try_latest(&self)->Result<Option<OwnedFrame>,&'static str> {
        Ok(self.try_latest_with_publish_id()?.map(|(_,frame)|frame))
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
        assert!(!a.consumer_active());
        a.set_consumer_active(true);
        assert!(a.consumer_active());
        a.set_consumer_active(false);
        assert!(!a.consumer_active());
        assert!(b.try_latest().unwrap().is_none());
        assert_eq!(a.latest_publish_id().unwrap(),None);
        assert_eq!(b.latest_publish_id().unwrap(),None);
        unsafe {a.store_header(PRODUCER_HEALTH_OFFSET,1);}
        assert!(a.latest_publish_id().is_err());
        a.clear_producer_health_for_new_process();
        assert_eq!(a.latest_publish_id().unwrap(),None);
        let before=a.generation();
        a.reset_publication();
        assert_eq!(a.generation(),before+1);
        assert_eq!(a.latest_publish_id().unwrap(),None);
        assert_eq!(b.generation(),0);
    }
    fn stage_mock_nv12(owner:&HqMappingOwner, slot:u32, publish_id:u32) {
        assert!(slot<2);
        let start=HEADER_BYTES+slot as usize*(SLOT_HEADER_BYTES+SLOT_CAPACITY);
        unsafe {
            ptr::write_bytes(owner.base.as_ptr().add(start),0,SLOT_HEADER_BYTES+24);
            owner.store_header(start,2); // both slots independently begin at seq 2
            owner.store_header(start+4,2); // NV12
            owner.store_header(start+8,4);
            owner.store_header(start+12,4);
            owner.store_header(start+24,4); // Y stride
            owner.store_header(start+28,4); // UV stride
            owner.store_header(start+36,0);
            owner.store_header(start+40,16);
            owner.store_header(start+48,16);
            owner.store_header(start+52,8);
            owner.store_header(start+60,24);
            ptr::write_bytes(owner.base.as_ptr().add(start+SLOT_HEADER_BYTES),
                             publish_id as u8,24);
            owner.store_header(20,slot);
            owner.store_header(24,publish_id);
        }
    }

    #[test]
    fn global_publish_id_advances_even_when_two_slots_have_equal_local_sequence() {
        let owner=HqMappingOwner::new().expect("HQ owner");
        stage_mock_nv12(&owner,0,1);
        let (first_id,first)=owner.try_latest_with_publish_id()
            .expect("valid").expect("first source frame");
        assert_eq!(first_id,1);
        assert_eq!(first.metadata.sequence,2);
        assert_eq!(first.data[0],1);
        stage_mock_nv12(&owner,1,2);
        let (second_id,second)=owner.try_latest_with_publish_id()
            .expect("valid").expect("second source frame");
        assert_eq!(second_id,2);
        // Each slot still has the SAME slot-local sequence. A renderer
        // deduplicating solely by sequence would freeze every other frame!
        assert_eq!(second.metadata.sequence,2);
        assert_ne!(first_id,second_id);
        assert_eq!(second.data[0],2);
        assert_eq!(second.data.len(),24);
    }

    #[test]
    fn header_check_does_not_alias_live_mutating_slots() {
        let owner=HqMappingOwner::new().unwrap();
        stage_mock_nv12(&owner,0,1);
        assert!(owner.try_latest().unwrap().is_some());
        unsafe {owner.store_header(HEADER_BYTES,3);}
        assert!(owner.try_latest_with_publish_id().unwrap().is_none());
        unsafe {owner.store_header(HEADER_BYTES,2);}
        assert!(owner.try_latest().unwrap().is_some());
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
