//! Manages the windowless USB detector shipped with the Windows USB test build.
//! The child reports connection state and shows its preview only when streaming.
use std::sync::Mutex;

static STATUS: Mutex<&'static str> = Mutex::new("starting");

pub(crate) fn status() -> &'static str {
    STATUS.lock().map(|state| *state).unwrap_or("unavailable")
}

#[cfg(target_os = "windows")]
mod native {
    use super::STATUS;
    use crate::ios_diagnostics::{self, CaptureFailure, Channel};
    use sha2::{Digest, Sha256};
    use std::{fs, io::{BufRead, BufReader, Cursor, Read, Write}, path::{Path, PathBuf},
        process::{ChildStdin, Command, Stdio}, sync::Mutex};
    use tauri::{AppHandle, Manager};

    const PAYLOAD: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/ios-usb-bundle.zip"));
    const PREVIEW: &str = "MirroLeapUsbPreview.exe";
    static INPUT: Mutex<Option<ChildStdin>> = Mutex::new(None);
    static VIDEO: Mutex<Option<(u32, (u32, u32))>> = Mutex::new(None);

    pub(super) fn video() -> Option<(u32, (u32,u32))> { VIDEO.lock().ok().and_then(|state| *state) }

    pub(super) fn control(start: bool) -> Result<(), String> {
        let mut input = INPUT.lock().map_err(|e| e.to_string())?;
        let status = super::status();
        if start && !matches!(status,"ready"|"interrupted") {
            return Err("USB 设备尚未就绪。".into());
        }
        input.as_mut().ok_or("USB 投屏组件未就绪")?
            .write_all(if start { b"S" } else { b"T" }).map_err(|e| e.to_string())?;
        set(if start { "connecting" } else { "stopping" });
        Ok(())
    }

    fn prepare(data_dir: &Path) -> Result<PathBuf, Box<dyn std::error::Error>> {
        let cache = data_dir.join("ios-usb").join(format!("{:x}", Sha256::digest(PAYLOAD)));
        fs::create_dir_all(&cache)?;
        let mut archive = zip::ZipArchive::new(Cursor::new(PAYLOAD))?;
        for index in 0..archive.len() {
            let mut member = archive.by_index(index)?;
            let name = member.enclosed_name().ok_or("Invalid USB runtime path")?;
            if member.is_symlink() || member.size() > 8 * 1024 * 1024 {
                return Err("Invalid USB runtime entry".into());
            }
            let file = cache.join(name);
            if member.is_dir() { fs::create_dir_all(&file)?; continue; }
            fs::create_dir_all(file.parent().ok_or("Invalid USB runtime path")?)?;
            let mut bytes = Vec::with_capacity(member.size() as usize);
            member.read_to_end(&mut bytes)?;
            let same_file = file.symlink_metadata().is_ok_and(|meta| {
                meta.file_type().is_file() && meta.len() == bytes.len() as u64
            }) && fs::read(&file).is_ok_and(|cached| cached == bytes);
            if !same_file {
                let temporary = file.with_extension(format!("tmp-{}", std::process::id()));
                fs::write(&temporary, &bytes)?;
                let moved = fs::rename(&temporary, &file);
                if moved.is_err() { let _ = fs::remove_file(&temporary); }
                moved?;
            }
        }
        Ok(cache.join(PREVIEW))
    }

    fn set(status: &'static str) {
        if let Ok(mut current) = STATUS.lock() { *current = status; }
        ios_diagnostics::observe(Channel::Usb, status, None);
        let video = match status {
            "streaming" => return,
            "connecting" => "waiting",
            "reconnecting" => "recovering",
            "interrupted" | "unavailable" => "interrupted",
            _ => "idle",
        };
        ios_diagnostics::observe(Channel::UsbVideo, video, None);
    }

    fn capture_failure(value: &str) -> Option<CaptureFailure> {
        let mut fields = value.split_ascii_whitespace();
        let failure = CaptureFailure {
            kind: fields.next()?.parse().ok()?,
            stage: fields.next()?.parse().ok()?,
            code: fields.next()?.parse().ok()?,
        };
        fields.next().is_none().then_some(failure)
    }

    fn observe_capture_message(line: &str) -> bool {
        if let Some(failure) = line.strip_prefix("failure ") {
            if let Some(failure) = capture_failure(failure) {
                ios_diagnostics::usb_failure(failure);
            }
            return true;
        }
        let phase = match line {
            "video receiving" => "receiving",
            "video paused" => "paused",
            "video recovering" => "recovering",
            _ => return false,
        };
        // These events describe the picture only. Keep both the preview and
        // the Bluetooth session around during a pause or in-place recovery.
        ios_diagnostics::observe(Channel::UsbVideo, phase, None);
        true
    }

    fn monitor(app: AppHandle, directory: PathBuf) -> Result<(), Box<dyn std::error::Error>> {
        let executable = prepare(&directory)?;
        let mut child = Command::new(executable)
            .arg(std::process::id().to_string())
            .stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::null())
            .spawn()?;
        let pid = child.id();
        *INPUT.lock().map_err(|_| "USB input unavailable")? = child.stdin.take();
        if let Some(stdout) = child.stdout.take() {
            for line in BufReader::new(stdout).lines().map_while(Result::ok) {
                // Accept only the state code, never forward device details.
                if observe_capture_message(line.trim()) { continue; }
                if let Some(dimensions) = line.strip_prefix("streaming ") {
                    if let Some(dimensions) = video_dimensions(dimensions) {
                        let first_frame = VIDEO.lock().is_ok_and(|mut state| {
                            state.replace((pid, dimensions)).is_none()
                        });
                        if first_frame {
                            if let Err(error) = crate::ios_device_frame::start_usb(&app,pid) {
                                eprintln!("[iOS USB frame] {error}");
                            }
                        }
                        set("streaming");
                    }
                    continue;
                }
                let status = match line.trim() {
                    "waiting" => "waiting",
                    "ready" => "ready",
                    "apple_driver" => "apple_driver",
                    "trust" | "authorization_pending" => "authorization_pending",
                    "detecting" => "detecting",
                    "unlock" => "unlock",
                    "capture_driver" => "capture_driver",
                    "busy" => "busy",
                    "connecting" => "connecting",
                    "reconnecting" => "reconnecting",
                    "interrupted" => "interrupted",
                    "stopping" => "stopping",
                    "unavailable" => "unavailable",
                    _ => continue,
                };
                close_preview(&app);
                set(status);
            }
        }
        let _ = child.wait();
        close_preview(&app);
        if let Ok(mut input) = INPUT.lock() { *input = None; }
        set("unavailable");
        Ok(())
    }

    fn close_preview(app: &AppHandle) {
        let was_streaming = VIDEO.lock().is_ok_and(|mut state| state.take().is_some());
        if was_streaming { crate::ios_device_frame::close_usb(app); }
    }

    fn video_dimensions(value: &str) -> Option<(u32,u32)> {
        let (width,height) = value.split_once(' ')?;
        let valid = |value: &str| value.parse::<u32>().ok().filter(|n| (1..=8192).contains(n));
        Some((valid(width)?,valid(height)?))
    }

    pub(super) fn start(app: &AppHandle) {
        if PAYLOAD.is_empty() { set("not_bundled"); return; }
        let Ok(directory) = app.path().app_local_data_dir() else { set("unavailable"); return; };
        let app = app.clone();
        std::thread::spawn(move || {
            if let Err(error) = monitor(app, directory) {
                eprintln!("[iOS USB] {error}");
                set("unavailable");
            }
        });
    }

    pub(super) async fn install_driver(app: &AppHandle) -> Result<(), String> {
        static INSTALL: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());
        let _install = INSTALL.try_lock().map_err(|_| "USB 驱动正在安装")?;
        if super::status() != "capture_driver" {
            return Err("请连接一台已信任且需要捕获驱动的 iPhone 或 iPad。".into());
        }
        let dir = app.path().app_local_data_dir().map_err(|e| e.to_string())?;
        tokio::task::spawn_blocking(move || {
            let executable = prepare(&dir).map_err(|e| e.to_string())?;
            let result = Command::new(executable).arg("--install-driver")
                .stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::null())
                .output().map_err(|e| e.to_string())?;
            if result.status.success() { return Ok(()); }
            let message = match String::from_utf8_lossy(&result.stdout).trim() {
                "one_device" => "请只连接一台已解锁并信任此电脑的 iPhone 或 iPad。",
                "no_driver_target" => "Windows 无法确认此设备的捕获驱动目标。请重新插拔数据线。",
                "detecting" => "USB 设备仍在识别中，请稍后重试。",
                "driver_cancelled" => "Windows 未批准 USB 驱动安装。",
                _ => "USB 捕获驱动安装失败。请重新插拔数据线后重试。",
            };
            Err(message.into())
        }).await.map_err(|e| e.to_string())?
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn failure_lines_accept_only_three_numeric_fields() {
            assert_eq!(capture_failure("3 5 -5").unwrap().code, -5);
            assert!(capture_failure("3 5 -5 device_serial").is_none());
            assert!(capture_failure("3 5 missing").is_none());
        }
    }
}

#[cfg(target_os = "windows")]
pub(crate) fn start(app: &tauri::AppHandle) {
    native::start(app);
}

#[cfg(target_os = "windows")]
pub(crate) fn video() -> Option<(u32,(u32,u32))> { native::video() }

#[cfg(target_os = "windows")]
pub(crate) fn control(start: bool) -> Result<(),String> { native::control(start) }

#[tauri::command]
pub fn ios_usb_control(window: tauri::Window, app: tauri::AppHandle, start: bool) -> Result<(),String> {
    if window.label() != "main" { return Err("Only the main window can start USB mirroring".into()); }
    #[cfg(target_os = "windows")]
    {
        if start { crate::ios_airplay::load_usb_display_size(&app)?; }
        control(start)
    }
    #[cfg(not(target_os = "windows"))]
    { let _ = (app,start); Err("USB mirroring requires Windows x64".into()) }
}

#[tauri::command]
pub async fn ios_usb_install_driver(window: tauri::Window, app: tauri::AppHandle) -> Result<(), String> {
    if window.label() != "main" { return Err("Only the main window can manage the USB driver".into()); }
    #[cfg(target_os="windows")]
    { native::install_driver(&app).await }
    #[cfg(not(target_os="windows"))]
    { let _ = app; Err("USB mirroring requires Windows x64".into()) }
}
