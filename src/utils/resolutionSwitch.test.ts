import { describe, it, expect } from 'vitest';
import { canApplyMirrorVideo, canQuickSwitchResolution } from './resolutionSwitch';
import { applyMirrorPreset, applyMirrorResolution } from './mirrorPresets';
import type { ScrcpyConfig } from '../hooks/useScrcpy';

const active: ScrcpyConfig = {
    device:'phone',sessionMode:'mirror',
    res:'1920',bitrate:8,fps:60,codec:'h264',record:false
};

describe('one-click mirroring video quality updates', () => {
    it('restarts once for a changed profile on an active phone, including bitrate-only changes', () => {
        for(const name of ['low-latency','quality','ultra'] as const){
            const next=applyMirrorPreset(active,name);
            expect(canApplyMirrorVideo(active,next,true,false))
                .toEqual({allowed:true,needsRestart:true});
        }
        expect(canApplyMirrorVideo(active,{...active,bitrate:9},true,false).needsRestart).toBe(true);
        expect(canApplyMirrorVideo(active,active,true,false))
            .toEqual({allowed:true,needsRestart:false});
        expect(canApplyMirrorVideo(active,applyMirrorPreset(active,'quality'),false,false))
            .toEqual({allowed:true,needsRestart:false});
    });

    it('rejects active recording before it can be interrupted or split', () => {
        const next=applyMirrorPreset(active,'quality');
        expect(canApplyMirrorVideo({...active,record:true},next,true,true))
            .toMatchObject({
                allowed:false,needsRestart:false,
                reason:expect.stringContaining('Recording in progress'),
            });
        expect(canApplyMirrorVideo(active,active,true,true).allowed).toBe(true);
    });

    it('validates video arguments and does not touch Android display settings', () => {
        for(const edited of [
            {...active,res:'adb shell wm size'},
            {...active,bitrate:900},
            {...active,bitrate:-1},
            {...active,fps:999},
            {...active,codec:'--help'},
        ]){
            expect(canApplyMirrorVideo(active,edited,true,false).allowed).toBe(false);
        }
        expect(canApplyMirrorVideo({...active,sessionMode:'camera'},applyMirrorPreset(active,'quality'),true,false))
            .toMatchObject({allowed:false,needsRestart:false});
    });

    it('selecting a manual 2K or 4K resolution also chooses matching bitrate/FPS', () => {
        expect(applyMirrorResolution(active,'2560')).toMatchObject(
            {res:'2560',bitrate:16,fps:60,codec:'h264'});
        expect(applyMirrorResolution(active,'3840')).toMatchObject(
            {res:'3840',bitrate:20,fps:30,codec:'h264'});
        expect(canQuickSwitchResolution('1280','1920','mirror',true,false))
            .toEqual({allowed:true,needsRestart:true});
        for (const size of ['0','800','1024','1280','1600','1920','2560','3840']){
            expect(canQuickSwitchResolution(size,'0','mirror',true,false).allowed).toBe(true);
        }
        expect(canQuickSwitchResolution('3840','0','mirror',true,true).allowed).toBe(false);
    });
});
