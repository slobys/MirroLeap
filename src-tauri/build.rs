fn main() {
    embed_windows_resource("MIRROLEAP_IOS_USB_BUNDLE", "ios-usb-bundle.zip");
    embed_windows_resource("MIRROLEAP_AIRPLAY_SDL_ADAPTER", "airplay-sdl-background.dll");
    tauri_build::build()
}

fn embed_windows_resource(variable: &str, file: &str) {
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        return;
    }
    println!("cargo:rerun-if-env-changed={variable}");
    let destination = std::path::PathBuf::from(std::env::var_os("OUT_DIR").unwrap()).join(file);
    if let Some(source) = std::env::var_os(variable) {
        println!("cargo:rerun-if-changed={}", std::path::Path::new(&source).display());
        std::fs::copy(source, destination).unwrap_or_else(|e| panic!("Cannot copy {file}: {e}"));
    } else {
        std::fs::write(destination, []).unwrap_or_else(|e| panic!("Cannot create {file}: {e}"));
    }
}
