import type { ScrcpyConfig } from '../hooks/useScrcpy';

export type StreamRestartResult = {
    applied: boolean;
    restored: boolean;
    activeConfig?: ScrcpyConfig;
    error?: string;
};

export type StreamRestartActions = {
    /** Subscribes to a scoped "running: false" event for THIS device. */
    listenForStop: (onStop: () => void) => Promise<() => void>;
    stop: () => Promise<void>;
    start: (config: ScrcpyConfig) => Promise<void>;
    /** Called AFTER the previous stop event has emitted the latest position. */
    getWindowPosition: () => { x: number; y: number } | undefined;
    timeoutMs?: number;
};

/**
 * scrcpy's max-size/bitrate/FPS/codec are startup-only. This transaction
 * performs a one-click guarded video-stream handoff, not an in-place decoder
 * mutation. Never launch the new process before the old monitor confirms
 * stopped, otherwise its cleanup can destroy the new session.
 *
 * The ADB pairing itself is not disconnected. User-facing switching UI must
 * remain busy until this completes. Recordings are rejected by the caller.
 */
export async function restartMirrorVideoStream(
    previous: ScrcpyConfig,
    requested: ScrcpyConfig,
    actions: StreamRestartActions,
): Promise<StreamRestartResult> {
    let notifyStopped!: () => void;
    let rejectStopped!: (err: Error) => void;
    const stopped = new Promise<void>((resolve,reject) => {
        notifyStopped = resolve;
        rejectStopped = reject;
    });
    // A slow native stop() may outlast the status timer. Attach a rejection
    // observer immediately, so a timeout cannot trigger an unhandled
    // rejection before we reach 'await stopped' after stop() completes.
    void stopped.catch(() => {});
    let unsubscribe: (() => void) | undefined;
    let timeout: ReturnType<typeof setTimeout> | undefined;
    let previousStopped = false;
    const withSavedPosition = (settings: ScrcpyConfig): ScrcpyConfig => {
        const pos = settings.rememberWindowPosition !== false
            ? actions.getWindowPosition() : undefined;
        return { ...settings,
            windowX: pos?.x ?? settings.windowX,
            windowY: pos?.y ?? settings.windowY };
    };
    try {
        // Register BEFORE stopping; Rust emits position BEFORE stopped.
        unsubscribe = await actions.listenForStop(notifyStopped);
        timeout = setTimeout(() => rejectStopped(new Error(
            'Previous mirroring stream did not confirm shutdown within the time limit'
        )), actions.timeoutMs ?? 10000);
        await actions.stop();
        await stopped;
        previousStopped = true;
        const activeConfig = withSavedPosition(requested);
        await actions.start(activeConfig);
        return { applied:true, restored:false, activeConfig };
    } catch (cause) {
        const error = String(cause);
        if (previousStopped) {
            try {
                const activeConfig = withSavedPosition(previous);
                await actions.start(activeConfig);
                return {applied:false, restored:true, activeConfig, error};
            } catch (restoreError) {
                return {applied:false,restored:false,error:`${error}; recovery failed: ${String(restoreError)}`};
            }
        }
        return {applied:false,restored:false,error};
    } finally {
        if (timeout) clearTimeout(timeout);
        try { unsubscribe?.(); } catch { /* Never mask the outcome. */ }
    }
}
