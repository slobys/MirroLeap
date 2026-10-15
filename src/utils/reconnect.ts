/** Exponential backoff for offline wireless ADB endpoints.
 * One in-flight connection per host; retries remain silent in the background. */
export function reconnectDelayMs(consecutiveFailures: number): number {
    const failures = Math.max(0, Math.min(6, Math.floor(consecutiveFailures)));
    return Math.min(300_000, 10_000 * 2 ** failures);
}

export function adbHistoryHost(endpoint: string): string | null {
    const match = /^((?:\d{1,3}\.){3}\d{1,3}):(\d{1,5})$/.exec(endpoint);
    if (!match) return null;
    const octets = match[1].split('.').map(Number);
    const port = Number(match[2]);
    if (octets.some(v => v > 255) || port < 1 || port > 65535) return null;
    return match[1];
}
