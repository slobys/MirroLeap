import type {ScrcpyConfig, MirrorPerformance} from '../hooks/useScrcpy';

/** Opt-in conservative FPS-guided heuristic, NOT network latency detection.
 * FPS depends on app activity and is never interpreted as packet loss.
 */
export type AutoQualityStep = {res: string; bitrate: number; reason: 'slow' | 'stable'};
const SIZES = ['800','1280','1920','2560'] as const;
const BITRATES = [3,4,8,16] as const;
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
        config.printFps === false || now - lastChangeAt < COOLDOWN_MS ||
        now - performance.observedAt > 8_000) return null;

    const index = SIZES.indexOf(config.res as typeof SIZES[number]);
    if (index < 0 || !Number.isInteger(config.bitrate) || (config.bitrate ?? 0) <= 0) return null;
    const samples = performance.samples.filter(v =>
        v.at <= now && v.at >= now - 120_000 && v.fps >= 1 && v.fps <= 240);
    if (samples.length < 10) return null;

    const cap = config.fps || 60;
    const low = samples.slice(-30);
    const span = low[low.length - 1].at - low[0].at;
    // Require meaningful ongoing frame production. A completely idle phone
    // should never be treated as congestion or trigger auto downscaling.
    if (low.length >= 10 && span >= 20_000 && Math.max(...low.map(x => x.fps)) >= 12 &&
        low.filter(x => x.fps < Math.min(23, cap * 0.45)).length >= Math.ceil(low.length * 0.85) &&
        low.filter(x => x.skipped > 0).length >= Math.ceil(low.length * 0.5)) {
        const bitrate = config.bitrate!;
        // Try cheaper bitrate adjustment before changing the video dimensions.
        if (bitrate > BITRATES[index] * 0.8 + 2) {
            return {res: SIZES[index], bitrate: Math.max(3, Math.round(bitrate * 0.75)), reason:'slow'};
        }
        if (index > 0) {
            return {res:SIZES[index - 1], bitrate:Math.min(bitrate, BITRATES[index - 1]), reason:'slow'};
        }
    }

    // Recovery is deliberately slower. Do not use a 30fps video as evidence
    // of congestion; only consider an upgrade when motion actually sustains
    // near the selected 60fps cap over a long interval.
    const stable = samples.slice(-70);
    if (cap >= 60 && stable.length >= 45 && stable[stable.length - 1].at - stable[0].at >= 65_000 &&
        stable.filter(s => s.fps >= cap * 0.92 && s.skipped <= 1).length >= stable.length * 0.90) {
        if (config.bitrate! < BITRATES[index]) {
            return {res:SIZES[index],bitrate:Math.min(BITRATES[index],config.bitrate! + 2),reason:'stable'};
        }
        if (index < SIZES.length - 1) {
            return {res:SIZES[index+1],bitrate:BITRATES[index+1],reason:'stable'};
        }
    }
    return null;
}
