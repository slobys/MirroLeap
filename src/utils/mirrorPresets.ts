import type { ScrcpyConfig } from '../hooks/useScrcpy';

export type MirrorPreset = 'balanced' | 'presentation' | 'low-latency' | 'quality';

/** The presets affect ONLY next-session video parameters. Never overwrite
 * device choice, wireless transport, audio, camera configuration, file
 * recording, per-device positions or the user's preferred phone border.
 */
export const MIRROR_PRESETS: Record<MirrorPreset, Pick<ScrcpyConfig, 'res' | 'bitrate' | 'fps'>> = {
    balanced: { res: '0', bitrate: 8, fps: undefined },
    presentation: { res: '1920', bitrate: 8, fps: 30 },
    'low-latency': { res: '1280', bitrate: 8, fps: 60 },
    quality: { res: '2560', bitrate: 16, fps: 60 },
};

export function applyMirrorPreset(config: ScrcpyConfig, preset: MirrorPreset): ScrcpyConfig {
    return { ...config, ...MIRROR_PRESETS[preset], sessionMode: 'mirror' };
}

export function activeMirrorPreset(config: ScrcpyConfig): MirrorPreset | 'custom' {
    for (const [name, preset] of Object.entries(MIRROR_PRESETS)) {
        if ((config.res || '0') === preset.res &&
            config.bitrate === preset.bitrate &&
            (config.fps || undefined) === preset.fps
        ) return name as MirrorPreset;
    }
    return 'custom';
}
