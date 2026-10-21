//! Optional MirroCreator HQ decoder runtime, independently downloaded ONCE.
//! The normal Windows EXE/MSI contains only the MirroLeap UI; this module has
//! no startup network activity. The user must explicitly enable HQ before
//! any download. The HTTPS archive must match the SHA256 compiled into the
//! signed HQ Preview application by Windows CI.
//!
//! The cached runtime is user-writable, out of Program Files, and does not
//! replace/modify any official scrcpy installation or Android server.

#![cfg(target_os = "windows")]

use std::{
    fs::{self,File},path::{Path,PathBuf},
    sync::atomic::{AtomicBool,Ordering},time::{SystemTime,UNIX_EPOCH},
};
use sha2::{Digest,Sha256};
use tauri::{AppHandle,Emitter,Manager};
use tokio::io::AsyncWriteExt;

const ROOT_NAME:&str="hq-runtime-v1";
const CACHE_MARKER:&str=".mirroleap-hq-installed";
const CLIENT_MARKER:&str="MirroLeap HQ decoded-frame v1 (experimental)";
const MAX_DOWNLOAD:u64=110*1024*1024;
const MAX_EXTRACT:u64=500*1024*1024;
const MAX_ENTRIES:usize=750;
static INSTALLING:AtomicBool=AtomicBool::new(false);

struct Installing;
impl Installing {
    fn acquire()->Result<Self,String> {
        INSTALLING.compare_exchange(false,true,Ordering::AcqRel,Ordering::Relaxed)
            .map_err(|_|"高清核心正在安装，请等待当前下载完成。".to_string())?;
        Ok(Self)
    }
}
impl Drop for Installing {
    fn drop(&mut self) {INSTALLING.store(false,Ordering::Release);}
}
struct TempRoot(PathBuf);
impl Drop for TempRoot {
    fn drop(&mut self) {let _=fs::remove_dir_all(&self.0);}
}
fn root(app:&AppHandle)->Result<PathBuf,String> {
    let home=app.path().app_local_data_dir()
        .map_err(|_|"无法获取高清核心的本地数据目录")?;
    Ok(home.join(ROOT_NAME))
}
fn valid_sha256(hash:&str)->bool {
    hash.len()==64 && hash.bytes().all(|b|b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}
fn verified_source()->Result<(&'static str,&'static str),String> {
    if option_env!("MIRROLEAP_HQ_PREVIEW").is_none() {
        return Err("只有 HQ Preview 支持一键获取可选高清核心。".into());
    }
    let url=option_env!("MIRROLEAP_HQ_CORE_URL")
        .ok_or("此构建没有提供已审核的 HQ 核心地址")?;
    let sha=option_env!("MIRROLEAP_HQ_CORE_SHA256")
        .ok_or("此构建没有提供 HQ 核心 SHA-256 校验值")?;
    if !url.starts_with("https://github.com/slobys/MirroLeap/releases/download/hq-preview-v")
        || !url.ends_with("/MirroLeap.HQ.Core_v1_x64.zip")
        || !valid_sha256(sha)
    {
        return Err("HQ 核心构建来源或校验值不受信任".into());
    }
    Ok((url,sha))
}
fn complete(dir:&Path)->bool {
    let marker=fs::read_to_string(dir.join(CACHE_MARKER)).ok();
    marker.as_deref().is_some_and(|v|v.starts_with("hq4.0-protocol-v1:") &&
        valid_sha256(v.trim().strip_prefix("hq4.0-protocol-v1:").unwrap_or("")))
        && ["scrcpy.exe","adb.exe","scrcpy-server"].iter()
            .all(|file|dir.join(file).is_file())
}
/// No network or new process. Used to find an already verified/cached
/// runtime when the user explicitly chooses source-frame magnification.
pub fn installed_client(app:&AppHandle)->Option<PathBuf> {
    let dir=root(app).ok()?;
    complete(&dir).then(||dir.join("scrcpy.exe"))
}
fn emit_progress(app:&AppHandle,stage:&str,percent:u64) {
    let _=app.emit("mirroleap-hq-core-progress",
        serde_json::json!({"stage":stage,"percent":percent.min(100)}));
}
fn validated_archive_entry(name:&str,unix_mode:Option<u32>,size:u64)
    ->Result<(),String>
{
    if name.contains('\\') || name.contains(':') ||
       name.starts_with('/') || !name.starts_with("scrcpy-hq-bin/") ||
       name.split('/').any(|part|part==".." || part=="." ||
                                 part.len()>180) ||
       unix_mode.is_some_and(|mode|mode & 0o170000 == 0o120000) ||
       size>MAX_EXTRACT
    {
        return Err("HQ runtime archive contains an unsafe path or link".into());
    }
    Ok(())
}
fn unpack(archive:&Path,stage:&Path,sha:&str)->Result<PathBuf,String> {
    let input=File::open(archive).map_err(|e|e.to_string())?;
    let mut zip=zip::ZipArchive::new(input)
        .map_err(|_|"HQ runtime archive is not a valid ZIP")?;
    if zip.len()==0 || zip.len()>MAX_ENTRIES {
        return Err("HQ runtime archive has an unexpected entry count".into());
    }
    let extract_to=stage.join("unpacked");
    fs::create_dir(&extract_to).map_err(|e|e.to_string())?;
    let mut total=0u64;
    for i in 0..zip.len() {
        let mut file=zip.by_index(i).map_err(|_|"HQ runtime ZIP entry is corrupt")?;
        let name=file.name().to_owned();
        validated_archive_entry(&name,file.unix_mode(),file.size())?;
        let safe=file.enclosed_name().ok_or("HQ runtime ZIP contains an unsafe path")?;
        total=total.checked_add(file.size()).ok_or("HQ runtime size overflow")?;
        if total>MAX_EXTRACT {return Err("HQ runtime expands beyond size limit".into());}
        let target=extract_to.join(safe);
        if file.is_dir() {
            fs::create_dir_all(&target).map_err(|e|e.to_string())?;
        } else {
            let parent=target.parent().ok_or("Invalid HQ runtime ZIP path")?;
            fs::create_dir_all(parent).map_err(|e|e.to_string())?;
            let mut output=fs::OpenOptions::new().create_new(true).write(true)
                .open(&target).map_err(|e|format!("Duplicate/unsafe HQ runtime file: {e}"))?;
            let written=std::io::copy(&mut file,&mut output).map_err(|e|e.to_string())?;
            if written!=file.size() {return Err("Truncated HQ runtime ZIP file".into());}
        }
    }
    let files=extract_to.join("scrcpy-hq-bin");
    if !["scrcpy.exe","adb.exe","scrcpy-server"].iter()
        .all(|name|files.join(name).is_file()) {
        return Err("HQ runtime is missing the patched client, adb or server".into());
    }
    fs::write(files.join(CACHE_MARKER),
        format!("hq4.0-protocol-v1:{sha}"))
        .map_err(|e|e.to_string())?;
    Ok(files)
}
async fn verify_client(exe:&Path)->Result<(),String> {
    let output=tokio::time::timeout(std::time::Duration::from_secs(7),
        crate::commands::create_command(exe).arg("--version").output())
        .await.map_err(|_|"HQ 实验核心启动超时")?
        .map_err(|e|format!("HQ 实验核心缺失 DLL 或无法运行: {e}"))?;
    if !output.status.success() || 
        !String::from_utf8_lossy(&output.stdout).contains(CLIENT_MARKER) ||
        !String::from_utf8_lossy(&output.stdout).contains("scrcpy 4.0") {
        return Err("下载的 HQ 核心没有通过 scrcpy 4.0 原始帧身份验证".into());
    }
    Ok(())
}
/// Called only by an explicit HQ toggle / a previously enabled HQ session.
/// Never called for ordinary scrcpy mirroring. Cache survives app updates.
pub async fn ensure(app:&AppHandle)->Result<PathBuf,String> {
    let _lock=Installing::acquire()?;
    // Never accept a cached binary in ordinary non-preview builds, even if
    // someone has manually placed files under the app-data directory.
    let (url,expected)=verified_source()?;
    if let Some(exe)=installed_client(app) {
        verify_client(&exe).await?;
        emit_progress(app,"ready",100);
        return Ok(exe);
    }
    let destination=root(app)?;
    let parent=destination.parent().ok_or("Missing HQ data folder")?;
    fs::create_dir_all(parent)
        .map_err(|e|format!("无法创建 HQ 缓存目录: {e}"))?;
    let now=SystemTime::now().duration_since(UNIX_EPOCH)
        .map_err(|_|"System clock not ready")?.as_nanos();
    let tmp=parent.join(format!(".hq-runtime-staging-{}-{}",std::process::id(),now));
    fs::create_dir(&tmp).map_err(|e|e.to_string())?;
    let _cleanup=TempRoot(tmp.clone());
    emit_progress(app,"download",0);
    let client=reqwest::Client::builder()
        .user_agent("MirroLeap-HQ-OptionalCore/1")
        .connect_timeout(std::time::Duration::from_secs(10))
        .timeout(std::time::Duration::from_secs(180))
        .build().map_err(|e|e.to_string())?;
    let mut response=client.get(url).send().await
        .map_err(|e|format!("HQ 核心下载失败: {e}"))?
        .error_for_status().map_err(|e|format!("HQ 核心下载失败: {e}"))?;
    if response.url().scheme()!="https" ||
       response.content_length().is_some_and(|len|len>MAX_DOWNLOAD) {
        return Err("HQ 核心下载目标不安全或过大".into());
    }
    let size=response.content_length();
    let archive=tmp.join("hq-core.zip");
    let mut output=tokio::fs::File::create(&archive).await
        .map_err(|e|e.to_string())?;
    let mut received=0u64;
    let mut digest=Sha256::new();
    while let Some(bytes)=response.chunk().await.map_err(|e|e.to_string())? {
        received=received.saturating_add(bytes.len() as u64);
        if received>MAX_DOWNLOAD {return Err("HQ 核心 ZIP 超过大小上限".into());}
        digest.update(&bytes);
        output.write_all(&bytes).await.map_err(|e|e.to_string())?;
        if let Some(total)=size.filter(|n|*n>0) {
            emit_progress(app,"download",(received*100/total).min(99));
        }
    }
    output.flush().await.map_err(|e|e.to_string())?;
    drop(output);
    if received==0 || size.is_some_and(|n|n!=received) ||
       format!("{:x}",digest.finalize())!=expected {
        return Err("HQ 核心 SHA-256 验证失败，不会安装或执行该文件".into());
    }
    emit_progress(app,"verify",99);
    let stage_for_worker=tmp.clone();
    let archive_for_worker=archive.clone();
    let hash=expected.to_owned();
    let staged=tokio::task::spawn_blocking(move||
        unpack(&archive_for_worker,&stage_for_worker,&hash))
        .await.map_err(|e|format!("HQ ZIP 解压任务失败: {e}"))??;
    verify_client(&staged.join("scrcpy.exe")).await?;
    // Do not replace or delete existing runtime folders if Windows is using
    // their DLLs. The only successful destination is a fresh, complete cache.
    if destination.exists() {
        if complete(&destination) {
            return Ok(destination.join("scrcpy.exe"));
        }
        return Err("已有损坏的 HQ 核心缓存，请关闭投屏后清理缓存再重试。".into());
    }
    fs::rename(&staged,&destination)
        .map_err(|e|format!("无法激活 HQ 缓存（可能需要退出占用的程序）: {e}"))?;
    emit_progress(app,"ready",100);
    Ok(destination.join("scrcpy.exe"))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn refuses_bad_integrity_or_zip_traversal() {
        assert!(valid_sha256(&"a".repeat(64)));
        assert!(!valid_sha256(&"x".repeat(64)));
        for path in ["../scrcpy-hq-bin/scrcpy.exe",
                     "scrcpy-hq-bin/../../Windows/notepad.exe",
                     "scrcpy-hq-bin/a\\b.exe",
                     "scrcpy-hq-bin/Z:/file.dll",
                     "/scrcpy-hq-bin/scrcpy.exe"] {
            assert!(validated_archive_entry(path,None,1).is_err(),"{path}");
        }
        assert!(validated_archive_entry("scrcpy-hq-bin/scrcpy.exe",None,100).is_ok());
        assert!(validated_archive_entry("scrcpy-hq-bin/link",Some(0o120777),1).is_err());
        assert!(validated_archive_entry("scrcpy-hq-bin/big.dll",None,MAX_EXTRACT+1).is_err());
    }
    #[test]
    fn never_has_download_source_in_an_unsigned_dev_build() {
        if option_env!("MIRROLEAP_HQ_PREVIEW").is_none() {
            assert!(verified_source().is_err());
        }
    }
}
