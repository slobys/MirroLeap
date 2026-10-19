import { describe, expect, it } from 'vitest';
import { applyMirrorPreset, activeMirrorPreset, MIRROR_PRESETS,
    mirrorVideoSummary, videoLoadWarning } from './mirrorPresets';
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
            expect(updated.codec).toBe('h264'); // Explicit preset chooses broadly compatible encoding
            expect(updated.renderDriver).toBeUndefined();
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
    it('uses 1080p-first performance profiles with distinct bandwidth and FPS tradeoffs', () => {
        expect(MIRROR_PRESETS).toEqual({
            balanced:{res:'1920',bitrate:8,fps:60,codec:'h264'},
            presentation:{res:'1920',bitrate:6,fps:30,codec:'h264'},
            'low-latency':{res:'1280',bitrate:4,fps:60,codec:'h264'},
            quality:{res:'1920',bitrate:12,fps:60,codec:'h264'},
        });
        const unique = new Set(Object.values(MIRROR_PRESETS).map(s => `${s.res}/${s.bitrate}/${s.fps}/${s.codec}`));
        expect(unique.size).toBe(4);
        expect(mirrorVideoSummary(applyMirrorPreset(base,'balanced'),'zh-CN'))
            .toContain('1080p · 8Mbps · 最高 60 帧 · H264');
        expect(videoLoadWarning(applyMirrorPreset(base,'balanced'),'zh-CN')).toBeNull();
    });

    it('never silently upgrades a custom or saved 2K/4K profile, and clearly warns about load', () => {
        expect(activeMirrorPreset(base)).toBe('custom');
        const old2K = {...base, res:'2560',bitrate:16,fps:60};
        const old4K = {...base, res:'3840'};
        expect(activeMirrorPreset(old2K)).toBe('custom');
        expect(videoLoadWarning(old2K,'zh-CN')).toMatch(/延迟/);
        expect(videoLoadWarning(old4K,'en')).toContain('latency');
        expect(videoLoadWarning({...base, res:'0'},'zh-CN')).toMatch(/原始分辨率/);
        expect(videoLoadWarning({...base, res:undefined},'zh-CN')).toMatch(/原始分辨率/);
        const manualCodec = {...MIRROR_PRESETS.balanced, sessionMode:'mirror', codec:'h265'};
        expect(activeMirrorPreset({...base,...manualCodec})).toBe('custom');
    });

    it('presets return mirror mode even after camera advanced-mode selection', () => {
        const result = applyMirrorPreset({...base, sessionMode:'camera'}, 'presentation');
        expect(result.sessionMode).toBe('mirror');
        expect(result.fps).toBe(30);
    });
});
