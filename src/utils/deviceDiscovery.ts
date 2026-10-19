/** Serializes ADB discovery across background polling, manual refresh and
 * reconnect callbacks. Calls for the same engine path share one request,
 * rather than starting parallel adb subprocesses.
 */
export function createSingleFlightDiscovery() {
    let active: {key:string; task:Promise<void>} | null = null;
    return async (key:string, work:()=>Promise<void>):Promise<void> => {
        while (active) {
            const ongoing = active;
            if (ongoing.key === key) return ongoing.task;
            // A user-selected engine change must not silently reuse a query
            // for the previous engine. Wait and run against the new key.
            try { await ongoing.task; } catch { /* next query still allowed */ }
        }
        let task:Promise<void>;
        try {
            task = Promise.resolve(work());
        } catch (error) {
            task = Promise.reject(error);
        }
        const own = {key,task};
        active = own;
        try { await task; } finally {
            if (active === own) active = null;
        }
    };
}
