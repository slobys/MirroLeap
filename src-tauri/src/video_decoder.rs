//! scrcpy HOST video decoding support (not the Android video *encoder* nor
//! SDL GPU rendering). scrcpy 5.0 introduced `--hwdec`; old scrcpy cores
//! MUST NOT receive this flag or they fail at startup.
//!
//! Hardware auto mode is already the default in scrcpy 5.0+. It falls back
//! internally to software when needed; never falsely promise a specific GPU
//! is capable merely because its command-line mode is advertised.
use serde::Serialize;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VideoDecoderSupport {
    pub host_os: String,
    pub scrcpy_version: Option<String>,
    pub supports_hwdec: bool,
    pub modes: Vec<String>,
    pub supports_print_fps: bool,
    pub diagnostics: String,
}

pub fn parse_scrcpy_version(version_output: &str) -> Option<(u32, u32, u32)> {
    // The actual --version output begins with 'scrcpy 5.0.1 <https://...>'.
    // Don't match library versions or unrelated numerals later in the text.
    for line in version_output.lines().take(5) {
        let mut parts = line.split_whitespace();
        if parts.next() != Some("scrcpy") { continue; }
        let raw = parts.next()?.trim_start_matches('v');
        let groups: Vec<&str> = raw.split('.').collect();
        if groups.len() != 2 && groups.len() != 3 { return None; }
        if groups.iter().any(|s| s.is_empty() || !s.bytes().all(|b| b.is_ascii_digit())) {
            return None;
        }
        let major = groups[0].parse().ok()?;
        let minor = groups[1].parse().ok()?;
        let patch = groups.get(2).map_or(Some(0), |s| s.parse().ok())?;
        return Some((major, minor, patch));
    }
    None
}

pub fn host_os() -> &'static str {
    if cfg!(target_os = "windows") { "windows" }
    else if cfg!(target_os = "linux") { "linux" }
    else if cfg!(target_os = "macos") { "macos" }
    else { "unknown" }
}

pub fn analyze_support(version_output: &str, help: &str, os: &str) -> VideoDecoderSupport {
    let version = parse_scrcpy_version(version_output);
    let is_v5 = version.is_some_and(|v| v.0 >= 5);
    let advertised = help.lines().any(|line| line.trim_start().starts_with("--hwdec="));
    let supports_hwdec = is_v5 && advertised;
    let mut modes = if supports_hwdec {
        vec!["auto".to_string(), "disabled".to_string()]
    } else {
        Vec::new()
    };
    if supports_hwdec {
        let selected = match os {
            "windows" => "d3d11va",
            "linux" => "vaapi",
            "macos" => "videotoolbox",
            _ => "",
        };
        // --help may advertise every OS mode. Only enable one appropriate
        // to this operating system; GPU/drivers are NOT inferred from help.
        if !selected.is_empty() && help.contains(selected) {
            modes.push(selected.to_string());
        }
    }
    let supports_print_fps = help.lines().any(|line|
        line.trim_start().starts_with("--print-fps")
    );
    let diagnostics = if version.is_none() {
        "Unrecognized scrcpy core version".to_string()
    } else if !is_v5 {
        "Hardware decoding requires scrcpy 5.0 or newer; legacy core unchanged".to_string()
    } else if !advertised {
        "This scrcpy build does not advertise --hwdec in --help".to_string()
    } else {
        "Modes detected in --help. Actual GPU use depends on graphics drivers and live codec".to_string()
    };
    VideoDecoderSupport {
        host_os: os.to_string(),
        scrcpy_version: version.map(|(major,minor,patch)| format!("{major}.{minor}.{patch}")),
        supports_hwdec,
        modes,
        supports_print_fps,
        diagnostics,
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DecoderDecision {
    // None = do not pass an --hwdec option; scrcpy 5.x selects auto,
    // scrcpy <5 preserves its software decoder behavior unchanged.
    pub flag: Option<String>,
    pub print_fps: bool,
    pub warning: Option<String>,
}

pub fn choose_decoder(
    requested: Option<&str>,
    print_fps: bool,
    caps: Option<&VideoDecoderSupport>,
    video_enabled: bool,
) -> DecoderDecision {
    let mode = requested.unwrap_or("auto").trim();
    if !video_enabled {
        return DecoderDecision { flag: None, print_fps: false, warning: None };
    }
    let fps = print_fps && caps.is_some_and(|support| support.supports_print_fps);
    if mode.is_empty() || mode == "auto" {
        return DecoderDecision { flag: None, print_fps: fps, warning: None };
    }
    match caps {
        Some(support) if support.modes.iter().any(|allowed| allowed == mode) =>
            DecoderDecision { flag: Some(format!("--hwdec={mode}")), print_fps: fps, warning: None },
        _ => DecoderDecision {
            flag: None,
            print_fps: fps,
            warning: Some(format!(
                "Requested decoder '{mode}' is unavailable in this scrcpy version or on this OS. Using the core's automatic/default decoding mode."
            )),
        },
    }
}

/// Only retry an explicitly forced hardware mode after the process exits
/// with a clear hardware-decoder error. Never restart for unrelated failures
/// such as ADB unauthorized, disconnected phones, or user stop.
pub fn is_hardware_decode_error(line: &str) -> bool {
    let lower = line.to_ascii_lowercase();
    let decoder_context = lower.contains("d3d11va") || lower.contains("vaapi") ||
        lower.contains("videotoolbox") || lower.contains("hwdec") ||
        lower.contains("hardware decod") || lower.contains("hardware device");
    decoder_context && ["fail", "error", "unavailable", "unsupported", "could not",
        "cannot", "unable", "invalid"].iter().any(|term| lower.contains(term))
}

/// Extract the ACTUAL decoder chosen by scrcpy. The requested decoder is not
/// a performance measurement or proof that a GPU was used.
pub fn observed_decoder(line: &str) -> Option<&'static str> {
    let lower = line.to_ascii_lowercase();
    let (_, suffix) = lower.split_once("video decoding:")?;
    let result = suffix.trim();
    if result.starts_with("d3d11va") { Some("d3d11va") }
    else if result.starts_with("vaapi") { Some("vaapi") }
    else if result.starts_with("videotoolbox") { Some("videotoolbox") }
    else if result.starts_with("software") || result.starts_with("sw") { Some("software") }
    else { Some("unknown") }
}

#[cfg(test)]
mod tests {
    use super::*;

    const V5_HELP: &str = "    --hwdec=mode\n        Possible values are auto, disabled, vaapi, d3d11va and videotoolbox\n    --print-fps\n    --render-driver=name\n";
    const V4_HELP: &str = "    --render-driver=name\n    --print-fps\n";

    #[test]
    fn version_parser_ignores_ffmpeg_dependency_numbers_and_malformed_versions() {
        assert_eq!(parse_scrcpy_version("scrcpy 5.0.1 <https://github.com/Genymobile/scrcpy>\n - libavcodec: 63.1.102"), Some((5,0,1)));
        assert_eq!(parse_scrcpy_version("scrcpy 4.1 <url>"), Some((4,1,0)));
        assert_eq!(parse_scrcpy_version("scrcpy v5.0"), Some((5,0,0)));
        assert_eq!(parse_scrcpy_version("FFmpeg: 5.0.1"), None);
        assert_eq!(parse_scrcpy_version("scrcpy 5.0.1-alpha.1"), None);
    }

    #[test]
    fn limits_gpu_modes_by_scrcpy_version_help_and_host_os() {
        let win = analyze_support("scrcpy 5.0.1", V5_HELP, "windows");
        assert!(win.supports_hwdec);
        assert_eq!(win.scrcpy_version.as_deref(), Some("5.0.1"));
        assert_eq!(win.modes, ["auto", "disabled", "d3d11va"].map(str::to_string).to_vec());
        assert!(win.supports_print_fps);
        assert_eq!(analyze_support("scrcpy 5.0.1", V5_HELP, "linux").modes,
            ["auto", "disabled", "vaapi"].map(str::to_string).to_vec());
        assert_eq!(analyze_support("scrcpy 5.0.1", V5_HELP, "macos").modes,
            ["auto", "disabled", "videotoolbox"].map(str::to_string).to_vec());
        assert!(analyze_support("scrcpy 4.1", V5_HELP, "windows").modes.is_empty());
        assert!(analyze_support("scrcpy 5.0", V4_HELP, "windows").modes.is_empty());
    }

    #[test]
    fn default_preserves_legacy_flags_and_software_fallback() {
        let old = analyze_support("scrcpy 4.1", V4_HELP, "windows");
        assert_eq!(choose_decoder(Some("auto"), false, Some(&old), true),
            DecoderDecision {flag:None, print_fps:false, warning:None});
        assert_eq!(choose_decoder(Some("d3d11va"), true, Some(&old), true).flag, None);
        assert!(choose_decoder(Some("d3d11va"), true, Some(&old), true).warning.is_some());
        let win = analyze_support("scrcpy 5.0.1", V5_HELP, "windows");
        assert_eq!(choose_decoder(Some("disabled"), true, Some(&win), true).flag.as_deref(), Some("--hwdec=disabled"));
        assert_eq!(choose_decoder(Some("d3d11va"), true, Some(&win), true).flag.as_deref(), Some("--hwdec=d3d11va"));
        assert!(choose_decoder(Some("d3d11va"), true, Some(&win), true).print_fps);
        assert_eq!(choose_decoder(Some("d3d11va"), true, Some(&win), false).flag, None);
        assert_eq!(choose_decoder(Some("--help"), false, Some(&win), true).flag, None);
    }

    #[test]
    fn hardware_error_detection_requires_decoder_context_not_adb_errors() {
        assert!(is_hardware_decode_error("ERROR: Could not initialize D3D11VA decoder"));
        assert!(is_hardware_decode_error("Hardware decoding failed; fallback is required"));
        assert!(!is_hardware_decode_error("ERROR: ADB device unauthorized"));
        assert!(!is_hardware_decode_error("INFO: Video decoding: d3d11va"));
        assert_eq!(observed_decoder("INFO: Video decoding: d3d11va"), Some("d3d11va"));
        assert_eq!(observed_decoder("INFO: Video decoding: software"), Some("software"));
        assert_eq!(observed_decoder("INFO: Renderer: direct3d"), None);
    }
}
