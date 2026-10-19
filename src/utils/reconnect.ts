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

/** Retry at most one approved endpoint per IP, and never reconnect a stale
 * port when ADB already reports the same phone's IP on a new TLS port.
 */
export function reconnectCandidates(history:string[], connected:string[]):string[] {
    const connectedHosts = new Set(connected.map(adbHistoryHost).filter(Boolean));
    const seenHosts = new Set<string>();
    const candidates:string[]=[];
    for (const endpoint of history) {
        const host=adbHistoryHost(endpoint);
        if (!host || seenHosts.has(host) || connectedHosts.has(host)) continue;
        seenHosts.add(host);
        candidates.push(endpoint);
    }
    return candidates;
}

/** Offer an explicit switch to a *verified connected* TLS port. The same IP
 * is only a hint, NOT device identity: never switch automatically. Multiple
 * candidates or an already connected old port are deliberately ambiguous.
 */
export function suggestedConnectedPort(selected:string, connected:string[]):string | null {
    const host=adbHistoryHost(selected);
    if (!host || connected.includes(selected)) return null;
    const matches=[...new Set(connected.filter(endpoint =>
        endpoint !== selected && adbHistoryHost(endpoint) === host))];
    return matches.length === 1 ? matches[0] : null;
}

/** A background reconnection can finish after the user clears remembered
 * addresses. Never re-add a removed endpoint from a stale async result.
 */
export function isReconnectStillApproved(
    address:string, savedHistory:string[], attemptEpoch:number, currentEpoch:number,
):boolean {
    return attemptEpoch === currentEpoch && savedHistory.includes(address) &&
        adbHistoryHost(address) !== null;
}
