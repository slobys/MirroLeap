import { describe, expect, it } from 'vitest';
import { historicWirelessStatus, mdnsServiceKind, normalizeWirelessEndpoint, safeDiagnosisCode, safeSameHostAlternatives } from './wirelessCenter';

describe('MirroLeap intelligent wireless connection safety and state', () => {
    it('accepts only exact unicast IPv4/port values and safely defaults legacy USB TCP mode', () => {
        expect(normalizeWirelessEndpoint(' 192.168.2.226 ')).toBe('192.168.2.226:5555');
        expect(normalizeWirelessEndpoint('10.0.0.2:42137')).toBe('10.0.0.2:42137');
        for (const value of ['', '-s 192.168.2.1', '10.0.0.1; echo bad', 'example.com:5555',
            '192.168.2.1:0', '192.168.2.1:65536', '0.0.0.0', '224.0.0.1',
            '255.255.255.255', '192.168.002.226:5555', '192.168.1.1:05555',
            '192.168.2.999:1234', '192.168.1.1/24', '192.168.1.1:1234:5678']) {
            expect(normalizeWirelessEndpoint(value)).toBeNull();
        }
    });

    it('suggests only unique same-host dynamically discovered connect ports', () => {
        expect(safeSameHostAlternatives({
            code: 'port_changed', address: '192.168.2.226:5555',
            alternatives: ['192.168.2.226:42341', '192.168.2.226:42341',
                '192.168.2.227:42341', '-x', '192.168.2.226:44559'],
        }, '192.168.2.226:5555')).toEqual([
            '192.168.2.226:42341', '192.168.2.226:44559',
        ]);
        expect(safeSameHostAlternatives(null,'192.168.2.226:5555')).toEqual([]);
    });

    it('never accepts untrusted dynamic translation keys', () => {
        expect(safeDiagnosisCode({code:'port_open',address:'192.168.1.5:12345',alternatives:[]})).toBe('port_open');
        expect(safeDiagnosisCode({code:'../../logs',address:'bad',alternatives:[]})).toBe('error');
        expect(safeDiagnosisCode(null)).toBe('error');
    });

    it('distinguishes pairing advertisements from connection ports and remembers offline status', () => {
        expect(mdnsServiceKind('_adb-tls-pairing._tcp')).toBe('pairing');
        expect(mdnsServiceKind('_adb-tls-connect._tcp')).toBe('connect');
        expect(mdnsServiceKind('_adb._tcp')).toBe('unknown');
        const address = '192.168.2.226:5555';
        expect(historicWirelessStatus(address,[address],[])).toBe('connected');
        expect(historicWirelessStatus(address,[],[{serial:address,state:'offline'}])).toBe('offline');
        expect(historicWirelessStatus(address,[],[{serial:address,state:'unauthorized'}])).toBe('unauthorized');
        expect(historicWirelessStatus(address,[],[])).toBe('remembered');
    });
});
