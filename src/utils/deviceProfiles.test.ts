import { describe, expect, it } from 'vitest';
import { applyDeviceProfile, selectDeviceConfig, profileFromConfig, readDeviceProfiles,
    rememberCompletedVideoForInactiveDevice, transitionDeviceSelection } from './deviceProfiles';
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
    it('gives newly seen phones safe 1080p defaults instead of carrying over 2K and recording', () => {
        const phoneA = {...base, device:'phone-a',res:'2560',bitrate:16,codec:'h265',
            record:true,autoQuality:true,clipboardSync:false,audioEnabled:false};
        const first = selectDeviceConfig(phoneA,undefined,'phone-a',false);
        expect(first).toMatchObject({res:'2560',bitrate:16,autoQuality:true,clipboardSync:false,record:false});
        const other = selectDeviceConfig(phoneA,undefined,'phone-b',true);
        expect(other).toMatchObject({device:'phone-b',res:'1920',bitrate:8,codec:'h264',fps:60,
            hardwareDecoder:'auto',audioEnabled:true,autoQuality:false,clipboardSync:true,record:false});
        const restored = selectDeviceConfig(other,profileFromConfig(phoneA),'phone-a',true);
        expect(restored).toMatchObject({device:'phone-a',res:'2560',bitrate:16,codec:'h265',
            autoQuality:true,clipboardSync:false,audioEnabled:false,record:false});
    });

    it('saves settings by actual config device during rapid A-to-B-to-C switches', () => {
        const phoneA={...base,device:'phone-a',res:'2560',bitrate:16,
            autoQuality:true,clipboardSync:false};
        const first=transitionDeviceSelection(phoneA,'phone-b',{});
        expect(first.profiles['phone-a']).toMatchObject({
            res:'2560',bitrate:16,autoQuality:true,clipboardSync:false});
        expect(first.config).toMatchObject({device:'phone-b',res:'1920',
            bitrate:8,autoQuality:false,clipboardSync:true,record:false});
        const second=transitionDeviceSelection(first.config,'phone-c',first.profiles);
        expect(second.profiles['phone-a']).toMatchObject({res:'2560',bitrate:16});
        expect(second.profiles['phone-b']).toMatchObject({
            res:'1920',bitrate:8,autoQuality:false,clipboardSync:true});
        expect(second.config.device).toBe('phone-c');
        const restore=transitionDeviceSelection(second.config,'phone-a',second.profiles);
        expect(restore.config).toMatchObject({device:'phone-a',res:'2560',
            bitrate:16,autoQuality:true,clipboardSync:false,record:false});
        expect(second.profiles['phone-a']).toMatchObject({res:'2560',bitrate:16});
    });
    it('rejects dangerous profile keys from a selected device serial', () => {
        const initial={'phone-a':profileFromConfig(base)};
        for (const target of ['__proto__','constructor','prototype']) {
            const output=transitionDeviceSelection(base,target,initial);
            expect(output.config).toBe(base);
            expect(output.profiles).toBe(initial);
        }
    });

    it('isolates delayed video restart completion to the original device profile', () => {
        const before = {
            'phone-a': {res:'1920',bitrate:8,fps:60,codec:'h264',deviceFrame:true},
            'phone-b': {res:'1280',bitrate:4,fps:60,codec:'h264',clipboardSync:false},
        };
        const result = rememberCompletedVideoForInactiveDevice(before,'phone-a',base,
            {res:'2560',bitrate:16,fps:60,codec:'h264'});
        expect(result['phone-a']).toEqual({res:'2560',bitrate:16,fps:60,codec:'h264',deviceFrame:true});
        expect(result['phone-b']).toEqual(before['phone-b']);
        expect(before['phone-a'].res).toBe('1920');
    });

    it('does not overwrite video settings changed while an old restart was in flight', () => {
        const updated = {'phone-a':{res:'1280',bitrate:4,fps:60,codec:'h264'}};
        expect(rememberCompletedVideoForInactiveDevice(updated,'phone-a',base,
            {res:'2560',bitrate:16,fps:60,codec:'h264'})).toBe(updated);
        expect(rememberCompletedVideoForInactiveDevice({},'phone-a',base,
            {res:'2560',bitrate:16,fps:60,codec:'h264'})).toEqual({});
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
        const extra = readDeviceProfiles('{"phone":{"clipboardSync":false,"autoQuality":true,"printFps":false,"record":true}}');
        expect(extra.phone).toEqual({clipboardSync:false,autoQuality:true});
        expect(parsed.b).not.toHaveProperty('recordPath');
    });
});
