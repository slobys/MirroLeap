/** A missing renderer-skip field is NOT proof of zero skipped frames.
 * Some scrcpy versions emit FPS without the optional skipped-frame statistic.
 */
export function measuredSkippedFrames(value: unknown): number | undefined {
    return typeof value === 'number' && Number.isInteger(value) &&
        value >= 0 && value <= 100_000 ? value : undefined;
}
