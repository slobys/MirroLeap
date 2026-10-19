/** Validate OS-provided local drop paths before submitting to the ADB bridge.
 * The Rust command independently validates each source and selected device.
 */
export function prepareTransferPaths(paths: unknown): {files: string[]; error?: string} {
    if (!Array.isArray(paths) || !paths.length) return {files:[],error:'No files selected'};
    if (paths.length > 30) return {files:[],error:'Select at most 30 files per transfer'};
    const files:string[]=[];
    for (const value of paths) {
        if (typeof value !== 'string' || !value || value.length > 4096 ||
            /[\u0000-\u001f]/.test(value) ||
            !(/^[A-Za-z]:[\\/]/.test(value) || value.startsWith('\\\\') || value.startsWith('/'))) {
            return {files:[],error:'Invalid local file path'};
        }
        if (!files.includes(value)) files.push(value);
    }
    return {files};
}
export function transferDisplayName(path: string): string {
    const value=path.replace(/\\/g,'/').split('/').pop() || 'file';
    return value.length > 60 ? value.slice(0,57) + '…' : value;
}

/** Accept only ADB-reported progress for this exact active device/file.
 * Unknown/unsupported ADB versions simply leave percentage unavailable.
 */
export function acceptedTransferProgress(
    payload: unknown, transferId: string, device: string, previous?: number,
): number | null {
    if (!payload || typeof payload !== 'object' || !transferId || !device) return null;
    const event = payload as Record<string, unknown>;
    if (event.transferId !== transferId || event.device !== device ||
        typeof event.percent !== 'number' || !Number.isInteger(event.percent) ||
        event.percent < 0 || event.percent > 100 ||
        (previous !== undefined && event.percent <= previous)) return null;
    return event.percent;
}
