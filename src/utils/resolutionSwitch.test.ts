import { describe, it, expect } from 'vitest';
import { canQuickSwitchResolution } from './resolutionSwitch';

describe('one-click mirroring resolution updates', () => {
    it('restarts only a running mirror session when its size really changes', () => {
        expect(canQuickSwitchResolution('1280','1920','mirror',true,false))
            .toEqual({ allowed: true, needsRestart: true });
        expect(canQuickSwitchResolution('1920','1920','mirror',true,false).needsRestart).toBe(false);
        expect(canQuickSwitchResolution('1280','1920','mirror',false,false).needsRestart).toBe(false);
        expect(canQuickSwitchResolution('1280','1920','camera',true,false).needsRestart).toBe(false);
    });
    it('never changes actual phone resolution and rejects dangerous values', () => {
        expect(canQuickSwitchResolution('adb shell wm size','0','mirror',true,false).allowed).toBe(false);
        expect(canQuickSwitchResolution('3840','0','mirror',true,true))
            .toMatchObject({ allowed: false, reason: expect.stringContaining('recording') });
    });
    it('accepts original and all UI presets', () => {
        for (const size of ['0','800','1024','1280','1600','1920','2560','3840']) {
            expect(canQuickSwitchResolution(size,'0','mirror',true,false).allowed).toBe(true);
        }
    });
});
