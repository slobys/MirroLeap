import type { ScrcpyConfig } from '../hooks/useScrcpy';

export type MirrorPreset = 'balanced' | 'presentation' | 'low-latency' | 'quality';

/**
 * scrcpy --max-size is the LONG EDGE of the Android display, not a 2K/4K
 * monitor preset. On high-resolution phones, res='0' sends the full native
 * screen and can create significant Wi-Fi encoding, network and decode delay.
 *
 * The 4 user-selectable scenarios control only the actual encoded video:
 * longest edge, bitrate, FPS ceiling and a broadly supported H.264 encoder.
 * Keep rotation, graphics renderer, GPU decoder, audio, frame, transport,
 * recording, screen orientation and window positions untouched. 2K and 4K
 * remain manual opt-ins with a latency warning rather than a preset default.
 */
export const MIRROR_PRESETS: Record<MirrorPreset,
    Pick<ScrcpyConfig, 'res' | 'bitrate' | 'fps' | 'codec'>> = {
    balanced:       { res: '1920', bitrate: 8,  fps: 60, codec: 'h264' },
    presentation:   { res: '1920', bitrate: 6,  fps: 30, codec: 'h264' },
    'low-latency':  { res: '1280', bitrate: 4,  fps: 60, codec: 'h264' },
    quality:        { res: '1920', bitrate: 12, fps: 60, codec: 'h264' },
};

export const MIRROR_PRESET_ORDER: MirrorPreset[] =
    ['balanced', 'presentation', 'low-latency', 'quality'];

export const MIRROR_PRESET_COPY: Record<MirrorPreset, {
    zh: string; en: string; hintZh: string; hintEn: string;
}> = {
    balanced: {
        zh: '均衡 · 日常推荐 (1080P)', en: 'Balanced · Recommended (1080p)',
        hintZh: '日常 Wi-Fi 推荐，兼顾清晰度与响应速度',
        hintEn: 'Recommended on Wi-Fi; balances sharpness and responsiveness',
    },
    presentation: {
        zh: '演示 · 文字清楚 (1080P)', en: 'Presentation · Sharp text (1080p)',
        hintZh: '讲解文字与课件时更省资源',
        hintEn: 'Lower load for slides, text and classroom demos',
    },
    'low-latency': {
        zh: '流畅 · 低延迟 (720P)', en: 'Smooth · Low latency (720p)',
        hintZh: '弱 Wi-Fi 或频繁操作时优先使用',
        hintEn: 'Lower load for weaker Wi-Fi and interactive control',
    },
    quality: {
        zh: '画质 · 视频观看 (1080P)', en: 'Quality · Video (1080p)',
        hintZh: '视频画面更清晰，但会占用更多带宽',
        hintEn: 'Sharper video with higher bandwidth demand',
    },
};

export function applyMirrorPreset(config: ScrcpyConfig, preset: MirrorPreset): ScrcpyConfig {
    // Only an explicit user action changes video settings. Old saved custom
    // configurations are never silently migrated to a new profile.
    return { ...config, ...MIRROR_PRESETS[preset], sessionMode: 'mirror' };
}

export function activeMirrorPreset(config: ScrcpyConfig): MirrorPreset | 'custom' {
    if (config.sessionMode !== 'mirror') return 'custom';
    for (const name of MIRROR_PRESET_ORDER) {
        const preset = MIRROR_PRESETS[name];
        if ((config.res || '0') === preset.res &&
            config.bitrate === preset.bitrate &&
            config.fps === preset.fps &&
            (config.codec || 'h264') === preset.codec
        ) return name;
    }
    return 'custom';
}

export function mirrorVideoSummary(config: ScrcpyConfig, locale: string): string {
    const zh = locale.startsWith('zh');
    const maxEdge: Record<string, string> = {
        '0': zh ? '原始分辨率' : 'Native size',
        '800': '480p', '1024': '576p', '1280': '720p',
        '1600': '900p', '1920': '1080p', '2560': '2K', '3840': '4K',
    };
    const resolution = maxEdge[config.res || '0'] || (config.res || '0');
    const fps = config.fps
        ? (zh ? `最高 ${config.fps} 帧` : `up to ${config.fps} fps`)
        : (zh ? '帧率自动' : 'automatic fps');
    const bitRate = `${config.bitrate ?? 8}Mbps`;
    return `${resolution} · ${bitRate} · ${fps} · ${(config.codec || 'h264').toUpperCase()}`;
}

/** Transport independent: any size beyond 1080p requires a conscious tradeoff.
 * Do not pretend the app has probed device encoder speed or real Wi-Fi latency.
 */
export function videoLoadWarning(config: ScrcpyConfig, locale: string): string | null {
    const requested = config.res || '0';
    const longEdge = Number(requested);
    if ((longEdge > 1920 && Number.isFinite(longEdge)) || requested === '0') {
        return locale.startsWith('zh')
            ? '原始分辨率、2K 或 4K 可能增加编码、Wi-Fi 传输与解码延迟。建议先试 1080P；高分辨率仅在设备和连接允许时使用。'
            : 'Native, 2K and 4K sizes can add encoding, Wi-Fi and decoding latency. Try 1080p first; use higher sizes only if your device and connection can handle them.';
    }
    return null;
}
