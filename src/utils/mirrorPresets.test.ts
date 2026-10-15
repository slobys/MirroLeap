import { describe, expect, it } from 'vitest';
import { applyMirrorPreset, activeMirrorPreset, MIRROR_PRESETS } from './mirrorPresets';
import type { ScrcpyConfig } from '../hooks/useScrcpy';

const base: ScrcpyConfig = {
    device: '192.168.2.226:5555', sessionMode: 'mirror', res: '0',
    bitrate: 8, audioEnabled: false, deviceFrame: true, record: true,
    recordPath: 'C:\\My videos', scrcpyPath: 'D:\\scrcpy', windowX: 345, windowY: 55,
    cameraId: '0', codec: 'h265',
};

describe('MirroLeap scenario profiles', () => {
    it('keeps exact safe defaults and preserves connected device and privacy-related options', () => {
        for (const name of Object.keys(MIRROR_PRESETS) as Array<keyof typeof MIRROR_PRESETS>) {
            const updated = applyMirrorPreset(base, name);
            expect(updated.device).toBe(base.device);
            expect(updated.audioEnabled).toBe(false);
            expect(updated.deviceFrame).toBe(true);
            expect(updated.record).toBe(true);
            expect(updated.recordPath).toBe(base.recordPath);
            expect(updated.scrcpyPath).toBe(base.scrcpyPath);
            expect(updated.windowX).toBe(345);
            expect(updated.cameraId).toBe('0');
            expect(updated.codec).toBe('h265');
            expect(activeMirrorPreset(updated)).toBe(name);
        }
    });
    it('presets do not mutate existing settings or trigger a new session', () => {
        const before = structuredClone(base);
        const changed = applyMirrorPreset(base, 'low-latency');
        expect(base).toEqual(before);
        expect(changed.res).toBe('1280');
        expect(changed.fps).toBe(60);
        expect(activeMirrorPreset({...changed, bitrate: 5})).toBe('custom');
    });
    it('presets return mirror mode even after camera advanced-mode selection', () => {
        const result = applyMirrorPreset({...base, sessionMode:'camera'}, 'presentation');
        expect(result.sessionMode).toBe('mirror');
        expect(result.fps).toBe(30);
    });
});
