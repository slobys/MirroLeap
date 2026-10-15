/** The same launch-time --max-size presets exposed by ControlPanel. */
export const MIRROR_SIZES = ['0', '800', '1024', '1280', '1600', '1920', '2560', '3840'] as const;

export function canQuickSwitchResolution(
    next: string,
    current: string,
    mode: string,
    running: boolean,
    recording: boolean
): { allowed: boolean; needsRestart: boolean; reason?: string } {
    if (!(MIRROR_SIZES as readonly string[]).includes(next)) {
        return { allowed: false, needsRestart: false, reason: 'Unsupported resolution' };
    }
    if (next === current) return { allowed: true, needsRestart: false };
    if (!running || mode !== 'mirror') return { allowed: true, needsRestart: false };
    if (recording) return {
        allowed: false, needsRestart: false,
        reason: 'Stop recording before changing mirror resolution'
    };
    return { allowed: true, needsRestart: true };
}
