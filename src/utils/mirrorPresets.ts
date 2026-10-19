import type { ScrcpyConfig } from '../hooks/useScrcpy';

export type MirrorPreset = 'low-latency' | 'balanced' | 'quality' | 'ultra';
export type MirrorVideoParams = Pick<ScrcpyConfig, 'res' | 'bitrate' | 'fps' | 'codec'>;

/**
 * scrcpy --max-size limits the LONGEST edge of the Android encoded stream.
 * No upscaling or device display/DPI changes are requested.
 *
 * These are starting points, NOT measured bandwidth or guaranteed FPS.
 * 2K/16M/60 matches the previously usable manual video target; 4K caps
 * FPS at 30 to avoid demanding 4K/60 from every phone and Wi-Fi network.
 */
export const MIRROR_PRESETS: Record<MirrorPreset, Required<MirrorVideoParams>> = {
    'low-latency': { res: '1280', bitrate: 4,  fps: 60, codec: 'h264' },
    balanced:      { res: '1920', bitrate: 8,  fps: 60, codec: 'h264' },
    quality:       { res: '2560', bitrate: 16, fps: 60, codec: 'h264' },
    ultra:         { res: '3840', bitrate: 20, fps: 30, codec: 'h264' },
};

export const MIRROR_PRESET_ORDER: MirrorPreset[] =
    ['low-latency', 'balanced', 'quality', 'ultra'];

export const MIRROR_PRESET_COPY: Record<MirrorPreset, {
    zh: string; en: string; hintZh: string; hintEn: string;
}> = {
    'low-latency': {
        zh: '720P · 流畅省流', en: '720p · Smooth',
        hintZh: '低带宽、弱 Wi-Fi 与快速操作',
        hintEn: 'Lower bandwidth for weaker Wi-Fi and interactive use',
    },
    balanced: {
        zh: '1080P · 均衡推荐', en: '1080p · Recommended',
        hintZh: '画质与响应速度兼顾',
        hintEn: 'Balanced sharpness and responsiveness',
    },
    quality: {
        zh: '2K · 高清', en: '2K · High definition',
        hintZh: '细节更清楚，适合已验证流畅的设备',
        hintEn: 'More detail on devices with sufficient headroom',
    },
    ultra: {
        zh: '4K · 极致画质', en: '4K · Maximum detail',
        hintZh: '提高画面尺寸上限，30 帧控负载；需设备支持',
        hintEn: 'Higher resolution limit, 30 fps cap; device-dependent',
    },
};

/** Only patch the video pipeline; never erase device, ADB, recording,
 * audio, window position, native frame, GPU decode or renderer options.
 */
export function applyMirrorPreset(config: ScrcpyConfig, preset: MirrorPreset): ScrcpyConfig {
    return { ...config, ...MIRROR_PRESETS[preset], sessionMode: 'mirror' };
}

export function applyMirrorResolution(config: ScrcpyConfig, size: string): ScrcpyConfig {
    const match = MIRROR_PRESET_ORDER.find(preset => MIRROR_PRESETS[preset].res === size);
    if (match) return applyMirrorPreset(config, match);
    // Less common manual sizes still get a sensible bitrate/FPS profile.
    // Native ('0') explicitly preserves the user's existing encoding choices.
    const uncommon: Record<string, MirrorVideoParams> = {
        '800':  { res: '800',  bitrate: 3, fps: 60, codec: 'h264' },
        '1024': { res: '1024', bitrate: 4, fps: 60, codec: 'h264' },
        '1600': { res: '1600', bitrate: 6, fps: 60, codec: 'h264' },
    };
    return { ...config, ...(uncommon[size] || { res: size }), sessionMode: 'mirror' };
}

export function videoParams(config: ScrcpyConfig): MirrorVideoParams {
    return {
        res: config.res || '0',
        bitrate: config.bitrate ?? 8,
        fps: config.fps,
        codec: config.codec || 'h264',
    };
}

export function activeMirrorPreset(config: ScrcpyConfig): MirrorPreset | 'custom' {
    if (config.sessionMode !== 'mirror') return 'custom';
    const selected = videoParams(config);
    for (const name of MIRROR_PRESET_ORDER) {
        const preset = MIRROR_PRESETS[name];
        if (selected.res === preset.res &&
            selected.bitrate === preset.bitrate &&
            selected.fps === preset.fps &&
            selected.codec === preset.codec) return name;
    }
    return 'custom';
}

export function mirrorVideoSummary(config: ScrcpyConfig, locale: string): string {
    const zh = locale.startsWith('zh');
    const label: Record<string, string> = {
        '0': zh ? '原始分辨率' : 'Native size',
        '800': '480p', '1024': '576p', '1280': '720p',
        '1600': '900p', '1920': '1080p', '2560': '2K', '3840': '4K',
    };
    const resolution = label[config.res || '0'] || (config.res || '0');
    const fps = config.fps
        ? (zh ? `最高 ${config.fps} 帧` : `up to ${config.fps} fps`)
        : (zh ? '帧率自动' : 'automatic fps');
    return `${resolution} · ${config.bitrate ?? 8}Mbps · ${fps} · ${(config.codec || 'h264').toUpperCase()}`;
}

export function videoLoadWarning(config: ScrcpyConfig, locale: string): string | null {
    const requested = config.res || '0';
    const longEdge = Number(requested);
    if (requested === '0' || (Number.isFinite(longEdge) && longEdge > 2560)) {
        return locale.startsWith('zh')
            ? '4K／原始分辨率是视频最长边上限，不会将手机画面放大到 4K。高帧率、高码率可能增加延迟，请按设备能力选择。'
            : '4K/native is a maximum video edge, not upscaling. High frame rates and bitrate can add latency; choose for your device.';
    }
    if (longEdge > 1920) {
        return locale.startsWith('zh')
            ? '2K 已可流畅使用时可以放心选用；若 Wi-Fi 变弱，可一键切回 1080P。'
            : 'Use 2K when it runs smoothly; switch to 1080p in one click if Wi-Fi weakens.';
    }
    return null;
}
