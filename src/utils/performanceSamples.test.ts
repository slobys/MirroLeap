import {describe, expect, it} from 'vitest';
import {measuredSkippedFrames} from './performanceSamples';

describe('native scrcpy frame-drop telemetry', () => {
    it('accepts only actual bounded skipped-frame counts', () => {
        expect(measuredSkippedFrames(0)).toBe(0);
        expect(measuredSkippedFrames(27)).toBe(27);
        expect(measuredSkippedFrames(100_000)).toBe(100_000);
    });
    it('does not invent zero for an absent or malformed measurement', () => {
        for (const value of [null, undefined, '0', false, -1, 3.5,
            100_001, Infinity, NaN, {}, []]) {
            expect(measuredSkippedFrames(value)).toBeUndefined();
        }
    });
});
