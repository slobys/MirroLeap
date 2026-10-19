import { describe, expect, it } from 'vitest';
import { applyDeviceProfile, profileFromConfig, readDeviceProfiles } from './deviceProfiles';
import type { ScrcpyConfig } from '../hooks/useScrcpy';

const base: ScrcpyConfig = {
    device: 'phone-a', sessionMode:'mirror', res:'1920', bitrate:8, fps:60, codec:'h264',
    audioEnabled:true, record:true, recordPath:'C:/private', scrcpyPath:'C:/scrcpy',
    deviceFrame:true, hardwareDecoder:'auto',
};
describe('device-scoped mirroring preferences', () => {
    it('keeps recording, connections and file paths outside profiles', () => {
        expect(profileFromConfig(base)).toEqual({
            res:'1920', bitrate:8, fps:60, codec:'h264', audioEnabled:true,
            deviceFrame:true, hardwareDecoder:'auto',
        });
    });
    it('restores display choices only for the selected phone', () => {
        const saved = profileFromConfig({...base, res:'2560',bitrate:16,audioEnabled:false});
        expect(applyDeviceProfile(base, saved, 'phone-b')).toMatchObject({
            device:'phone-b',res:'2560',bitrate:16,audioEnabled:false,
            record:true,recordPath:'C:/private',
        });
        expect(base.res).toBe('1920');
    });
    it('rejects malformed and unsafe persisted values and legacy native resolution', () => {
        expect(readDeviceProfiles('{')).toEqual({});
        expect(readDeviceProfiles('[]')).toEqual({});
        const parsed = readDeviceProfiles(JSON.stringify({
            a:{res:'0',bitrate:-9,fps:10,codec:'not-valid',audioEnabled:false,recordPath:'malicious',deviceFrame:true},
            b:{res:'1280',bitrate:4,fps:60,codec:'h264',hardwareDecoder:'auto'},
        }));
        expect(parsed.a).toEqual({audioEnabled:false,deviceFrame:true});
        expect(parsed.b).toEqual({res:'1280',bitrate:4,fps:60,codec:'h264',hardwareDecoder:'auto'});
        expect(parsed.b).not.toHaveProperty('recordPath');
    });
});
