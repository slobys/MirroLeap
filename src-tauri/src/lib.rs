mod commands;
mod transfer_progress;
// Experimental decoded-frame IPC parser; not enabled in production mirroring.
#[allow(dead_code)]
mod hq_protocol;
#[allow(dead_code)]
mod hq_geometry;
#[allow(dead_code)]
mod hq_reference_render;
#[allow(dead_code)]
mod hq_pipeline;
#[cfg(target_os = "windows")]
#[allow(dead_code)]
mod hq_bridge_windows;
#[cfg(target_os = "windows")]
#[allow(dead_code)]
mod hq_cpu_lens_windows;
#[cfg(target_os = "windows")]
mod hq_runtime;
mod app_update;
mod android_rotation;
mod capture_compositor;
mod video_decoder;
mod core_manager;
mod frame_layout;
#[cfg(target_os = "windows")]
mod device_frame;
#[cfg(target_os = "windows")]
mod native_live_lens;
#[cfg(target_os = "windows")]
mod native_click_effect;
#[cfg(target_os = "windows")]
mod win_chrome;
// Windows-only: drag the borderless scrcpy window with the mouse
// (Ctrl+Alt+Shift+W).
#[cfg(target_os = "windows")]
mod grab;
// Cross-platform global OS shortcuts (e.g. Ctrl+Alt+Shift+C to recentre the
// active mirror window), shared with grab.rs's Windows-only drag toggle.
mod shortcuts;
use std::collections::HashMap;
use std::sync::Mutex;
use tauri::Manager;
use tokio::process::Child;

#[cfg(target_os = "linux")]
use std::os::unix::process::CommandExt;

pub struct ScrcpyState {
    pub processes: Mutex<HashMap<String, Child>>,
    /// Explicit, per-device opt-in decoded-frame sessions. Never persisted.
    #[cfg(target_os = "windows")]
    pub hq_sessions: Mutex<HashMap<String,std::sync::Arc<hq_bridge_windows::HqMappingOwner>>>,
    /// ADB binary resolved for each active mirror (supports custom core paths).
    pub adb_paths: Mutex<HashMap<String, String>>,
    /// Current recording file per device; shared with user-stop and natural
    /// scrcpy exits so we report saved files only after the process closes.
    pub recording_paths: Mutex<HashMap<String, std::path::PathBuf>>,
    pub active_device: Mutex<Option<String>>,
    /// A raw window position captured by `stop_scrcpy` right before it kills
    /// the process (the window is still exactly where the user left it at
    /// that instant), handed off to the run_scrcpy monitor loop so it can
    /// apply its already-learned decoration offset instead of persisting its
    /// own periodic sample, which can be a few seconds stale. See
    /// `resolve_window_pos_to_persist` in commands.rs.
    pub final_capture_hint: Mutex<HashMap<String, (i32, i32)>>,
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    // Fix for white screen on Linux (Wayland/NVIDIA)
    #[cfg(target_os = "linux")]
    {
        if std::env::var("WEBKIT_DISABLE_COMPOSITING_MODE").is_err() {
            std::env::set_var("WEBKIT_DISABLE_COMPOSITING_MODE", "1");
        }

        // Workaround for AppImage blank UI on Fedora/Wayland
        // Preload the host's libwayland-client.so.0 to prevent conflicts with bundled version
        // Checking whether it is an AppImage or not by checking APPDIR environment variable
        if std::env::var("APPDIR").is_ok() && std::env::var("WAYLAND_DISPLAY").is_ok() {
            let preload = std::env::var("LD_PRELOAD").unwrap_or_default();
            if !preload.contains("libwayland-client.so.0") {
                // checking host native libwayland-client.so.0 is loaded or not by checking LD_PRELOAD environment variable
                let paths = [
                    "/usr/lib64/libwayland-client.so.0",
                    "/usr/lib/x86_64-linux-gnu/libwayland-client.so.0",
                    "/usr/lib/libwayland-client.so.0",
                ];
                for path in paths {
                    // if host native libwayland-client.so.0 is found it will be loaded instead of bundled version
                    if std::path::Path::new(path).exists() {
                        let mut new_preload = preload;
                        if !new_preload.is_empty() {
                            new_preload.push(':');
                        }
                        new_preload.push_str(path);
                        std::env::set_var("LD_PRELOAD", &new_preload);

                        let current_exe = std::env::current_exe().unwrap_or_else(|_| {
                            std::path::PathBuf::from(std::env::args().next().unwrap())
                        });
                        let mut cmd = std::process::Command::new(current_exe);
                        cmd.args(std::env::args().skip(1));
                        let _ = cmd.exec();
                        break;
                    }
                }
            }
        }
    }

    let builder = tauri::Builder::default()
        .plugin(tauri_plugin_shell::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_clipboard_manager::init())
        .plugin(tauri_plugin_process::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        // The global-shortcut plugin must be registered on the builder (its
        // setup wires the OS hotkey manager on the main thread). Individual
        // shortcuts are registered below in setup.
        .plugin(shortcuts::plugin());

    builder
        .setup(|app| {
            app.manage(ScrcpyState {
                processes: Mutex::new(HashMap::new()),
                #[cfg(target_os = "windows")]
                hq_sessions: Mutex::new(HashMap::new()),
                adb_paths: Mutex::new(HashMap::new()),
                recording_paths: Mutex::new(HashMap::new()),
                active_device: Mutex::new(None),
                final_capture_hint: Mutex::new(HashMap::new()),
            });

            // Ctrl+Alt+Shift+C (recentre the active mirror window) on every
            // platform, plus Ctrl+Alt+Shift+W (drag the borderless window) on
            // Windows. Non-fatal if a shortcut fails to register.
            if let Err(e) = shortcuts::register(app.handle()) {
                eprintln!("[shortcuts] failed to register global shortcuts: {e}");
            }

            // Windows-only: start the drag-follow thread backing Ctrl+Alt+Shift+W.
            #[cfg(target_os = "windows")]
            grab::register(app.handle());

            // Show splashscreen instantly
            if let Some(splash_window) = app.get_webview_window("splashscreen") {
                splash_window.show().unwrap();
            }

            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::greet,
            commands::check_scrcpy,
            commands::get_devices,
            commands::adb_connect,
            commands::adb_usb_to_wireless,
            commands::diagnose_adb_connection,
            commands::get_mdns_devices,
            commands::adb_pair,
            commands::adb_shell,
            commands::push_file,
            commands::install_apk,
            commands::kill_adb,
            commands::run_scrcpy,
            commands::stop_scrcpy,
            commands::device_frame_control,
            commands::get_mirrocreator_lens_status,
            commands::ensure_hq_runtime,
            commands::get_app_install_kind,
            commands::open_official_community_link,
            commands::device_frame_ack,
            commands::device_frame_page_ready,
            commands::device_frame_lens_aperture,
            commands::device_frame_capture_ack,
            commands::recenter_scrcpy_window,
            commands::set_active_device,
            commands::download_scrcpy,
            commands::list_scrcpy_options,
            commands::get_render_drivers,
            commands::get_video_decoder_support,
            commands::get_videos_dir,
            commands::save_report,
            commands::save_device_screenshot,
            commands::preview_saved_screenshot,
            commands::presentation_set_mode,
            commands::presentation_get_mode,
            commands::presentation_refresh_snapshot,
            commands::save_annotated_screenshot,
            commands::get_scrcpy_bin_dir,
            commands::run_terminal_command,
            commands::check_scrcpy_update,
            app_update::check_mirroleap_update,
            app_update::open_official_release_link,
            app_update::download_official_release_asset,
            app_update::install_signed_update,
            app_update::check_mirroleap_preview_update,
            close_splashscreen,
            get_app_version
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

#[tauri::command]
fn get_app_version(app: tauri::AppHandle) -> String {
    app.package_info().version.to_string()
}

#[tauri::command]
async fn close_splashscreen(window: tauri::Window) {
    // Get the main window
    if let Some(main_window) = window.get_webview_window("main") {
        // Show the main window
        main_window.show().unwrap();
    }
    // Close the splashscreen window
    if let Some(splash_window) = window.get_webview_window("splashscreen") {
        splash_window.close().unwrap();
    }
}
