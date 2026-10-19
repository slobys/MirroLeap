import {describe, it, expect} from 'vitest';
import {chooseAutoQualityStep} from './autoQuality';
import type {ScrcpyConfig, MirrorPerformance} from '../hooks/useScrcpy';

const NOW = 500_000;
const base:ScrcpyConfig={device:'a',sessionMode:'mirror',res:'2560',bitrate:16,fps:60,
    codec:'h264',autoQuality:true,printFps:true};
const sample = (fps:number,count:number,stepMs:number,skipped=fps<23 ? 2 : 0):MirrorPerformance => {
    const samples = Array.from({length:count},(_,i)=>({fps,skipped,at:NOW-(count-1-i)*stepMs}));
    return {fps,skipped,observedAt:NOW,samples};
};
const calc=(performance:MirrorPerformance, overrides:Partial<ScrcpyConfig>={}, rest:Partial<Parameters<typeof chooseAutoQualityStep>[0]>={})=>
    chooseAutoQualityStep({config:{...base,...overrides},performance,now:NOW,
        lastChangeAt:100_000,running:true,recording:false,switching:false,...rest});
describe('conservative opt-in auto quality',()=>{
    it('never guesses from missing, stale or static-screen FPS',()=>{
        expect(calc(sample(2,16,2000))).toBeNull();
        expect(calc(sample(15,5,2000))).toBeNull();
        expect(calc({...sample(15,20,2000),observedAt:NOW-12000})).toBeNull();
        expect(calc(sample(15,20,2000),{autoQuality:false})).toBeNull();
        // An app deliberately rendering at 17 FPS isn't proof of congestion.
        expect(calc(sample(17,20,2000,0))).toBeNull();
    });
    it('does not alter recording or a concurrent restart, and enforces cooldown',()=>{
        const x=sample(17,20,2000);
        expect(calc(x,{}, {recording:true})).toBeNull();
        expect(calc(x,{}, {switching:true})).toBeNull();
        expect(calc(x,{}, {lastChangeAt:NOW-5000})).toBeNull();
        expect(calc(x,{}, {running:false})).toBeNull();
    });
    it('first reduces bitrate and only then steps down resolution',()=>{
        const x=sample(17,20,2000);
        expect(calc(x)).toEqual({res:'2560',bitrate:12,reason:'slow'});
        expect(calc(x,{res:'1920',bitrate:6})).toEqual({res:'1280',bitrate:4,reason:'slow'});
        expect(calc(x,{res:'800',bitrate:3})).toBeNull();
        // scrcpy FPS reports arrive roughly once per second, not every 2s.
        // The observation window must be wide enough to recognize a sustained dip.
        expect(calc(sample(17,30,1000))).toEqual({res:'2560',bitrate:12,reason:'slow'});
    });
    it('works when manual FPS display is disabled and ignores pre-restart samples',()=>{
        const sustained = sample(17,30,1000);
        expect(calc(sustained,{printFps:false})).toEqual({res:'2560',bitrate:12,reason:'slow'});
        expect(calc(sustained,{}, {lastChangeAt:NOW-185_000})).toEqual({res:'2560',bitrate:12,reason:'slow'});
        // Samples from before a recent stream handoff cannot justify another change.
        expect(calc(sustained,{}, {lastChangeAt:NOW-181_000})).toEqual({res:'2560',bitrate:12,reason:'slow'});
        expect(calc({...sustained,samples:sustained.samples.map(v=>({...v,skipped:NaN}))})).toBeNull();
    });
    it('requires sustained high observed motion before upgrading, never to 4K',()=>{
        expect(calc(sample(59,70,1000),{res:'1920',bitrate:8})).toEqual({res:'2560',bitrate:16,reason:'stable'});
        expect(calc(sample(59,70,1000),{res:'2560',bitrate:16})).toBeNull();
        expect(calc(sample(30,70,1000),{res:'1920',bitrate:8})).toBeNull();
    });
});
