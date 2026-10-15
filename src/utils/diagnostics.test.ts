import { describe, expect, it } from 'vitest';
import { safeDiagnosticSettings, redactDiagnosticText } from './diagnostics';

const storage = (data: Record<string, string>) => ({
    getItem: (key: string) => data[key] ?? null,
});

describe('privacy-first local diagnostics', () => {
    it('only exports harmless settings and excludes Wi-Fi, paths, serials, tokens', () => {
        const state = storage({
            scrcpy_theme: 'ultraviolet',
            scrcpy_color_mode: 'dark',
            scrcpy_window_positions: '{"adb-serial":{"x":2,"y":3}}',
            unrelated_password: 'keep-secret',
            scrcpy_config: JSON.stringify({
                sessionMode: 'mirror', res: '1920', fps: 60, bitrate: 8,
                scrcpyPath: 'C:\\Users\\Someone\\private',
                recordPath: 'D:\\Records',
                device: '192.168.1.200:5555', apiKey: 'secret', deviceFrame: true,
            }),
        });
        const snapshot = safeDiagnosticSettings(state);
        expect(snapshot).toEqual({
            scrcpy_theme: 'ultraviolet', scrcpy_color_mode: 'dark',
            sessionMode: 'mirror', res: '1920', fps: 60, bitrate: 8, deviceFrame: true,
        });
        expect(JSON.stringify(snapshot)).not.toMatch(/secret|Someone|192\.168/);
    });

    it('ignores malformed configs and caps untrusted text', () => {
        expect(safeDiagnosticSettings(storage({scrcpy_config: '{invalid', scrcpy_theme: '../escape'})))
            .toEqual({});
        const redacted = redactDiagnosticText(
            'adb 192.168.1.15:5555 password=testing token:supersecret C:\\Users\\Alice\\Desktop\\file.log'
        );
        expect(redacted).not.toContain('192.168.1.15');
        expect(redacted).not.toContain('supersecret');
        expect(redacted).not.toContain('Alice');
        expect(redacted).toContain('[IP address]');
        expect(redactDiagnosticText('x'.repeat(450_000))).toHaveLength(400_000);
    });
});
