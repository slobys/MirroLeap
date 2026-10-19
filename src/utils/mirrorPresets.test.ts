import { describe, expect, it } from 'vitest';
import { applyMirrorPreset, applyMirrorResolution, activeMirrorPreset, MIRROR_PRESETS,
    MIRROR_PRESET_ORDER, mirrorVideoSummary, videoLoadWarning } from './mirrorPresets';
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
    it('has four unique 720p/1080p/2K/4K video presets with safe load-aware caps', () => {
        expect(MIRROR_PRESETS).toEqual({
            'low-latency':{res:'1280',bitrate:4,fps:60,codec:'h264'},
            balanced:{res:'1920',bitrate:8,fps:60,codec:'h264'},
            quality:{res:'2560',bitrate:16,fps:60,codec:'h264'},
            ultra:{res:'3840',bitrate:20,fps:30,codec:'h264'},
        });
        expect(MIRROR_PRESET_ORDER.map(x=>MIRROR_PRESETS[x].res))
            .toEqual(['1280','1920','2560','3840']);
        for(const name of MIRROR_PRESET_ORDER){
            const setting = applyMirrorPreset(base,name);
            expect(activeMirrorPreset(setting)).toBe(name);
            expect(applyMirrorResolution(base,MIRROR_PRESETS[name].res))
                .toEqual(setting);
        }
        expect(mirrorVideoSummary(applyMirrorPreset(base,'balanced'),'zh-CN'))
            .toContain('1080p · 8Mbps · 最高 60 帧 · H264');
    });

    it('preserves custom saved choices and offers 2K without wrongly calling it laggy', () => {
        expect(activeMirrorPreset(base)).toBe('custom');
        const old2K = {...base, res:'2560',bitrate:12,fps:60};
        expect(activeMirrorPreset(old2K)).toBe('custom');
        expect(videoLoadWarning(old2K,'zh-CN')).toContain('2K 已可流畅');
        expect(videoLoadWarning({...base, res:'3840'},'en')).toContain('latency');
        expect(videoLoadWarning({...base, res:'0'},'zh-CN')).toContain('原始分辨率');
        const manualCodec = {...MIRROR_PRESETS.balanced, sessionMode:'mirror',codec:'h265'};
        expect(activeMirrorPreset({...base,...manualCodec})).toBe('custom');
    });

    it('pairs uncommon manual sizes with useful video parameters without upscaling Android', () => {
        expect(applyMirrorResolution(base,'1600')).toMatchObject(
            {res:'1600',bitrate:6,fps:60,codec:'h264'});
        expect(applyMirrorResolution(base,'800')).toMatchObject(
            {res:'800',bitrate:3,fps:60});
        expect(applyMirrorResolution(base,'0')).toMatchObject(
            {res:'0',bitrate:8,codec:'h265'});
        expect(applyMirrorPreset({...base,sessionMode:'camera'},'ultra')).toMatchObject(
            {sessionMode:'mirror',res:'3840',bitrate:20,fps:30});
    });
});
