import type {ScrcpyConfig, MirrorPerformance} from '../hooks/useScrcpy';

/** Opt-in conservative FPS-guided heuristic, NOT network latency detection.
 * FPS depends on app activity and is never interpreted as packet loss.
 */
export type AutoQualityStep = {res: string; bitrate: number; reason: 'slow' | 'stable'};
// Include every non-4K resolution offered by advanced settings. Small
// intermediate steps reduce visible disruption when auto-recovering quality.
export const AUTO_QUALITY_SIZES = ['800','1024','1280','1600','1920','2560'] as const;
const BITRATES = [3,4,4,6,8,16] as const;
const COOLDOWN_MS = 180_000;

export function chooseAutoQualityStep(input: {
    config: ScrcpyConfig;
    performance?: MirrorPerformance;
    now: number;
    lastChangeAt: number;
    running: boolean;
    recording: boolean;
    switching: boolean;
}): AutoQualityStep | null {
    const {config, performance, now, lastChangeAt, running, recording, switching} = input;
    if (!config.autoQuality || !running || recording || switching ||
        config.sessionMode !== 'mirror' || !performance ||
        !Number.isFinite(now) || !Number.isFinite(lastChangeAt) ||
        lastChangeAt > now || now - lastChangeAt < COOLDOWN_MS ||
        !Number.isFinite(performance.observedAt) ||
        performance.observedAt > now || now - performance.observedAt > 8_000) return null;

    const index = AUTO_QUALITY_SIZES.indexOf(config.res as typeof AUTO_QUALITY_SIZES[number]);
    if (index < 0 || !Number.isInteger(config.bitrate) || (config.bitrate ?? 0) <= 0) return null;
    // Auto mode requests its own FPS telemetry regardless of the optional
    // manual print-FPS switch. Never use stale samples from before a restart.
    const samples = performance.samples.filter(v =>
        Number.isFinite(v.at) && v.at <= now && v.at >= now - 120_000 &&
        v.at >= lastChangeAt && Number.isFinite(v.fps) && v.fps >= 1 && v.fps <= 240 &&
        (v.skipped === undefined ||
            (Number.isInteger(v.skipped) && v.skipped >= 0 && v.skipped <= 100_000)));
    if (samples.length < 10) return null;

    const cap = config.fps || 60;
    const low = samples.slice(-30);
    const span = low[low.length - 1].at - low[0].at;
    // Require meaningful ongoing frame production. A completely idle phone
    // should never be treated as congestion or trigger auto downscaling.
    if (low.length >= 10 && span >= 20_000 && Math.max(...low.map(x => x.fps)) >= 12 &&
        low.filter(x => x.fps < Math.min(23, cap * 0.45)).length >= Math.ceil(low.length * 0.85) &&
        low.filter(x => typeof x.skipped === 'number' && x.skipped > 0).length >= Math.ceil(low.length * 0.5)) {
        const bitrate = config.bitrate!;
        // Try cheaper bitrate adjustment before changing the video dimensions.
        if (bitrate > BITRATES[index] * 0.8 + 2) {
            return {res: AUTO_QUALITY_SIZES[index], bitrate: Math.max(3, Math.round(bitrate * 0.75)), reason:'slow'};
        }
        if (index > 0) {
            return {res:AUTO_QUALITY_SIZES[index - 1], bitrate:Math.min(bitrate, BITRATES[index - 1]), reason:'slow'};
        }
    }

    // Recovery is deliberately slower. Do not use a 30fps video as evidence
    // of congestion; only consider an upgrade when motion actually sustains
    // near the selected 60fps cap over a long interval.
    const stable = samples.slice(-70);
    if (cap >= 60 && stable.length >= 45 && stable[stable.length - 1].at - stable[0].at >= 65_000 &&
        stable.filter(s => s.fps >= cap * 0.92 &&
            typeof s.skipped === 'number' && s.skipped <= 1).length >= stable.length * 0.90) {
        if (config.bitrate! < BITRATES[index]) {
            return {res:AUTO_QUALITY_SIZES[index],bitrate:Math.min(BITRATES[index],config.bitrate! + 2),reason:'stable'};
        }
        if (index < AUTO_QUALITY_SIZES.length - 1) {
            return {res:AUTO_QUALITY_SIZES[index+1],bitrate:BITRATES[index+1],reason:'stable'};
        }
    }
    return null;
}
