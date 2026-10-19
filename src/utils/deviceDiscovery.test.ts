import {describe,it,expect,vi} from 'vitest';
import {createSingleFlightDiscovery} from './deviceDiscovery';

function deferred() {
    let resolve!:()=>void;
    const promise=new Promise<void>(ok=>{resolve=ok;});
    return {promise,resolve};
}

describe('ADB discovery single-flight scheduler',()=>{
    it('coalesces overlapping background and manual refresh for the same engine',async()=>{
        const gate=createSingleFlightDiscovery();
        const first=deferred();
        const work=vi.fn(()=>first.promise);
        const run1=gate('adb-path-A',work);
        const run2=gate('adb-path-A',work);
        const run3=gate('adb-path-A',work);
        expect(work).toHaveBeenCalledTimes(1);
        first.resolve();
        await Promise.all([run1,run2,run3]);
        await gate('adb-path-A',work);
        expect(work).toHaveBeenCalledTimes(2);
    });
    it('waits for an old query before starting with a newly selected engine',async()=>{
        const gate=createSingleFlightDiscovery();
        const first=deferred();
        const workA=vi.fn(()=>first.promise);
        const workB=vi.fn(async()=>{});
        const a=gate('old-core',workA);
        const b=gate('new-core',workB);
        expect(workB).not.toHaveBeenCalled();
        first.resolve();
        await Promise.all([a,b]);
        expect(workB).toHaveBeenCalledTimes(1);
    });
    it('releases its lock after a failed ADB query',async()=>{
        const gate=createSingleFlightDiscovery();
        await expect(gate('core',async()=>{throw new Error('adb unavailable');}))
            .rejects.toThrow('adb unavailable');
        const work=vi.fn(async()=>{});
        await gate('core',work);
        expect(work).toHaveBeenCalledTimes(1);
    });
});
