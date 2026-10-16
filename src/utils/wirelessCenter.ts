import type { PendingAdbDevice, WirelessDiagnosis } from '../hooks/useScrcpy';

export type WirelessOperation = 'connect' | 'diagnose' | 'pair' | 'usb';
export type WirelessPhase = 'connecting' | 'diagnosing' | 'connected' | 'paired' | 'needs_action' | 'failed';

export interface WirelessFeedback {
    phase: WirelessPhase;
    operation: WirelessOperation;
    address: string;
    diagnosis?: WirelessDiagnosis;
    details?: string;
}

export interface WirelessOperationResult {
    success: boolean;
    address?: string;
    message?: unknown;
    diagnosis?: WirelessDiagnosis;
}

/** Manual connect accepts only unicast IPv4 and a single optional TCP port.
 * No hostnames, flags, extra ADB arguments or entire LAN subnets. An IP without
 * a port means legacy USB-enabled TCP/5555, NOT Android 11+ pairing.
 */
export function normalizeWirelessEndpoint(input: string): string | null {
    const match = /^(\d{1,3}(?:\.\d{1,3}){3})(?::(\d{1,5}))?$/.exec(input.trim());
    if (!match) return null;
    const octets = match[1].split('.');
    if (octets.some(part => (part.length > 1 && part.startsWith('0')) || Number(part) > 255)) {
        return null;
    }
    const first = Number(octets[0]);
    if (first === 0 || first >= 224) return null;
    const port = match[2] === undefined ? 5555 : Number(match[2]);
    if (port < 1 || port > 65535 || (match[2]?.length > 1 && match[2].startsWith('0'))) {
        return null;
    }
    return octets.join('.') + ':' + port;
}

/** Never render an arbitrary server-provided string as a dynamic i18n key. */
const DIAGNOSIS_CODES = new Set([
    'invalid', 'connected', 'unauthorized', 'offline',
    'port_changed', 'port_open', 'refused', 'timeout', 'unreachable', 'error',
]);

export function safeDiagnosisCode(report: WirelessDiagnosis | null | undefined): string {
    return report && DIAGNOSIS_CODES.has(report.code) ? report.code : 'error';
}

export function safeSameHostAlternatives(report: WirelessDiagnosis | null | undefined, attempted: string): string[] {
    const host = normalizeWirelessEndpoint(attempted)?.split(':')[0];
    if (!host) return [];
    const seen = new Set<string>();
    const results: string[] = [];
    for (const candidate of report?.alternatives ?? []) {
        const normalized = normalizeWirelessEndpoint(candidate);
        if (normalized && normalized.split(':')[0] === host && normalized !== attempted && !seen.has(normalized)) {
            seen.add(normalized);
            results.push(normalized);
        }
        if (results.length === 5) break;
    }
    return results;
}

/** mDNS connect advertisements are not necessarily authorization: only the
 * phone's actual pairing service should receive a pairing code. */
export function mdnsServiceKind(service: string): 'pairing' | 'connect' | 'unknown' {
    if (service.includes('_adb-tls-pairing._tcp')) return 'pairing';
    if (service.includes('_adb-tls-connect._tcp')) return 'connect';
    return 'unknown';
}

export function historicWirelessStatus(address: string, ready: string[], pending: PendingAdbDevice[]): 'connected' | 'unauthorized' | 'offline' | 'remembered' {
    if (ready.includes(address)) return 'connected';
    const state = pending.find(d => d.serial === address)?.state;
    if (state === 'unauthorized') return 'unauthorized';
    if (state === 'offline') return 'offline';
    return 'remembered';
}
