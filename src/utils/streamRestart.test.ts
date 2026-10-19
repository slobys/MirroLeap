import { describe, expect, it, vi } from 'vitest';
import { restartMirrorVideoStream } from './streamRestart';
import type { ScrcpyConfig } from '../hooks/useScrcpy';

const previous: ScrcpyConfig = {
    device:'192.168.1.32:5555', sessionMode:'mirror',
    res:'1920',bitrate:8,fps:60,codec:'h264',
    deviceFrame:true,alwaysOnTop:true,audioEnabled:false,
    rememberWindowPosition:true,record:false,
};
const next: ScrcpyConfig = {...previous,res:'2560',bitrate:16};

describe('one-click automatic stream handoff', () => {
    it('subscribes before stopping, waits for stop acknowledgement and keeps real frame/window preferences', async () => {
        const events: string[]=[];
        let onStopped!:()=>void;
        let signalStop!:()=>void;
        const stoppedCommandInvoked = new Promise<void>(resolve => { signalStop=resolve; });
        const start=vi.fn(async (_c:ScrcpyConfig)=>{ events.push('start'); });
        const unsubscribe=vi.fn();
        const promise = restartMirrorVideoStream(previous,next,{
            listenForStop:async cb=>{ events.push('listen');onStopped=cb;return unsubscribe; },
            stop:async ()=>{ events.push('stop');signalStop(); },
            start,
            getWindowPosition:()=>({x:345,y:78}),
        });
        await stoppedCommandInvoked;
        expect(events).toEqual(['listen','stop']);
        expect(start).not.toHaveBeenCalled();
        onStopped();
        const result=await promise;
        expect(result).toMatchObject({applied:true,restored:false});
        expect(events).toEqual(['listen','stop','start']);
        expect(start).toHaveBeenCalledWith(expect.objectContaining({
            res:'2560',bitrate:16,windowX:345,windowY:78,
            deviceFrame:true,alwaysOnTop:true,audioEnabled:false,record:false,
        }));
        expect(unsubscribe).toHaveBeenCalledExactlyOnceWith();
    });

    it('recovers the old video after a failed new launch without leaving stale settings', async () => {
        let stopSignal!:()=>void;
        const start=vi.fn().mockRejectedValueOnce(new Error('2K encoder unavailable'))
            .mockResolvedValueOnce(undefined);
        const outcome=await restartMirrorVideoStream(previous,next,{
            listenForStop:async cb=>{stopSignal=cb;return () => {}; },
            stop:async()=>{stopSignal();},
            start,
            getWindowPosition:()=>({x:200,y:300}),
        });
        expect(outcome).toMatchObject({
            applied:false,restored:true,
            activeConfig:expect.objectContaining({res:'1920',bitrate:8,windowX:200}),
            error:expect.stringContaining('2K encoder unavailable'),
        });
        expect(start).toHaveBeenCalledTimes(2);
        expect(start.mock.calls[0][0]).toMatchObject({res:'2560'});
        expect(start.mock.calls[1][0]).toMatchObject({res:'1920'});
    });

    it('does not start a second scrcpy if old stream stop was never acknowledged', async () => {
        const stop=vi.fn(async()=>{});
        const start=vi.fn(async()=>{});
        const unsubscribe=vi.fn();
        const outcome=await restartMirrorVideoStream(previous,next,{
            listenForStop:async()=>unsubscribe,
            stop,start,
            getWindowPosition:()=>undefined,
            timeoutMs:15,
        });
        expect(outcome).toMatchObject({
            applied:false,restored:false,
            error:expect.stringContaining('did not confirm shutdown'),
        });
        expect(stop).toHaveBeenCalledOnce();
        expect(start).not.toHaveBeenCalled();
        expect(unsubscribe).toHaveBeenCalledOnce();
    });

    it('keeps a recorder untouched if stopping returns an error', async () => {
        const start=vi.fn();
        const result=await restartMirrorVideoStream(previous,next,{
            listenForStop:async()=>() => {},
            stop:async()=>{throw Error('native stop refused');},
            start,
            getWindowPosition:()=>undefined,
        });
        expect(result.applied).toBe(false);
        expect(result.restored).toBe(false);
        expect(start).not.toHaveBeenCalled();
    });
});
