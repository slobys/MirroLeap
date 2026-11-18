/** Recovery is deliberately narrower than ADB reconnection. Do not silently
 * restart screen recorders or a user-closed native scrcpy window. */
export function isRecoverableMirrorSession(
    config: { sessionMode?: string; record?: boolean } | undefined,
): boolean {
    return Boolean(config && config.sessionMode === 'mirror' && !config.record);
}

export type RecoveryBudget = { firstAt: number; attempts: number };

/** Maximum two unexpected process restarts in two minutes per device.
 * No unbounded crash loops, even when a phone remains visible to ADB. */
export function nextMirrorRecovery(
    now: number, previous: RecoveryBudget | undefined,
): { budget: RecoveryBudget; delayMs: number } | null {
    const budget = !previous || now - previous.firstAt > 120000
        ? { firstAt: now, attempts: 0 }
        : { ...previous };
    if (budget.attempts >= 2) return null;
    budget.attempts += 1;
    return { budget, delayMs: budget.attempts === 1 ? 2500 : 7000 };
}
