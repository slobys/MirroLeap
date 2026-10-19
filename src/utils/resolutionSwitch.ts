import type { ScrcpyConfig } from '../hooks/useScrcpy';
import { videoParams } from './mirrorPresets';

/** The same launch-time --max-size choices exposed by ControlPanel. */
export const MIRROR_SIZES = ['0', '800', '1024', '1280', '1600', '1920', '2560', '3840'] as const;

export type SwitchDecision = { allowed: boolean; needsRestart: boolean; reason?: string };

export function canApplyMirrorVideo(
    previous: ScrcpyConfig,
    next: ScrcpyConfig,
    running: boolean,
    recording: boolean,
): SwitchDecision {
    const target = videoParams(next);
    if (!(MIRROR_SIZES as readonly string[]).includes(target.res || '0')) {
        return { allowed: false, needsRestart: false, reason: 'Unsupported resolution' };
    }
    if (!Number.isInteger(target.bitrate) || (target.bitrate ?? 0) < 1 ||
        (target.bitrate ?? 0) > 50) {
        return { allowed: false, needsRestart: false, reason: 'Unsupported bitrate' };
    }
    if (target.fps !== undefined && ![30, 60, 90, 120].includes(target.fps)) {
        return { allowed: false, needsRestart: false, reason: 'Unsupported FPS cap' };
    }
    if (!['h264', 'h265', 'av1', 'vp8', 'vp9'].includes(target.codec || '')) {
        return { allowed: false, needsRestart: false, reason: 'Unsupported video codec' };
    }
    const current = videoParams(previous);
    const changed = (['res', 'bitrate', 'fps', 'codec'] as const)
        .some(key => current[key] !== target[key]);
    if (!changed) return { allowed: true, needsRestart: false };
    if (!running) return { allowed: true, needsRestart: false };
    if (previous.sessionMode !== 'mirror') {
        return { allowed: false, needsRestart: false, reason: 'Active session is not screen mirroring' };
    }
    // A recorder's MKV must not be split/abruptly closed without explicit
    // consent. The recording settings have their own opt-in restart action.
    if (recording) {
        return { allowed: false, needsRestart: false,
            reason: 'Recording in progress; stop recording before changing video quality' };
    }
    return { allowed: true, needsRestart: true };
}

/** Compatibility wrapper for the legacy size-only callers/tests. */
export function canQuickSwitchResolution(
    next: string, current: string, mode: string, running: boolean, recording: boolean
): SwitchDecision {
    const old: ScrcpyConfig = { device:'', sessionMode:mode, res:current, bitrate:8, codec:'h264' };
    const requested: ScrcpyConfig = { ...old, res:next };
    return canApplyMirrorVideo(old, requested, running, recording);
}
