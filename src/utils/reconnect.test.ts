import { describe, expect, it } from 'vitest';
import { adbHistoryHost, reconnectDelayMs, reconnectCandidates } from './reconnect';

describe('wireless reconnect scheduler', () => {
    it('backs off after repeated failures and caps at five minutes', () => {
        expect(reconnectDelayMs(0)).toBe(10000);
        expect(reconnectDelayMs(1)).toBe(20000);
        expect(reconnectDelayMs(2)).toBe(40000);
        expect(reconnectDelayMs(8)).toBe(300000);
    });
    it('does not hammer an old TLS port if the same IP is already connected', () => {
        const stored=['192.168.2.226:5555','192.168.2.226:42341',
            '192.168.2.150:5555','192.168.2.151:5555','invalid'];
        const online=['192.168.2.226:42341','192.168.2.151:63210'];
        expect(reconnectCandidates(stored, online)).toEqual(['192.168.2.150:5555']);
        expect(reconnectCandidates(['192.168.2.226:5555','192.168.2.226:42341'],[]))
            .toEqual(['192.168.2.226:5555']);
    });
    it('uses one reconnect attempt per valid device IP', () => {
        expect(adbHistoryHost('192.168.2.226:5555')).toBe('192.168.2.226');
        expect(adbHistoryHost('192.168.2.226:42341')).toBe('192.168.2.226');
        expect(adbHistoryHost('192.168.2.226')).toBe(null);
        expect(adbHistoryHost('not-an-ip:1234')).toBe(null);
        expect(adbHistoryHost('192.168.2.999:5555')).toBe(null);
        expect(adbHistoryHost('192.168.2.226:70000')).toBe(null);
    });
});
