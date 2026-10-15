/** Opt-in diagnostics only: NEVER dump the entire localStorage object.
 * It may contain device serials, recent Wi-Fi hosts, recording paths or
 * unrelated credentials from other integrations. Only a small allowlist of
 * non-identifying MirroLeap UI/video settings is useful for troubleshooting.
 */
export function safeDiagnosticSettings(storage: Pick<Storage, 'getItem'>) {
    const snapshot: Record<string, string | number | boolean> = {};
    for (const key of ['scrcpy_theme', 'scrcpy_color_mode']) {
        const value = storage.getItem(key);
        if (value && /^[A-Za-z0-9_-]{1,32}$/.test(value)) snapshot[key] = value;
    }
    let config: unknown;
    try {
        config = JSON.parse(storage.getItem('scrcpy_config') || '{}');
    } catch {
        config = {};
    }
    if (config && typeof config === 'object' && !Array.isArray(config)) {
        const record = config as Record<string, unknown>;
        for (const name of [
            'sessionMode', 'res', 'codec', 'audioCodec', 'renderDriver',
            'bitrate', 'fps', 'deviceFrame', 'audioEnabled', 'vsync',
        ]) {
            const value = record[name];
            if (typeof value === 'boolean') snapshot[name] = value;
            else if (typeof value === 'number' && Number.isFinite(value) && value >= 0 && value < 100_000)
                snapshot[name] = value;
            else if (typeof value === 'string' && /^[A-Za-z0-9_-]{1,32}$/.test(value))
                snapshot[name] = value;
        }
    }
    return snapshot;
}

/** Best-effort redaction, not an absolute privacy guarantee. The generated
 * JSON is local and should still be reviewed before sharing with others.
 */
export function redactDiagnosticText(raw: string): string {
    return raw.slice(0, 400_000)
        .replace(/\b(?:\d{1,3}\.){3}\d{1,3}(?::\d{2,5})?\b/g, '[IP address]')
        .replace(/\b[A-Za-z]:\\(?:[^\s\\/:*?"<>|]+\\)*[^\s\\/:*?"<>|]*/g, '[local path]')
        .replace(/\b(password|passwd|api[_-]?key|authorization|bearer|token)\s*[:=]\s*\S+/gi, '$1=[redacted]');
}
