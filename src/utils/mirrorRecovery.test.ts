import { describe, expect, it } from 'vitest';
import { isRecoverableMirrorSession, nextMirrorRecovery } from './mirrorRecovery';

describe('unexpected scrcpy exit recovery', () => {
    it('only relaunches a normal, non-recording mirror session', () => {
        expect(isRecoverableMirrorSession(undefined)).toBe(false);
        expect(isRecoverableMirrorSession({sessionMode:'mirror'})).toBe(true);
        expect(isRecoverableMirrorSession({sessionMode:'mirror',record:true})).toBe(false);
        expect(isRecoverableMirrorSession({sessionMode:'camera'})).toBe(false);
        expect(isRecoverableMirrorSession({sessionMode:'desktop'})).toBe(false);
    });

    it('stops after two retries within two minutes and resets later', () => {
        const first = nextMirrorRecovery(1000, undefined)!;
        expect(first.delayMs).toBe(2500);
        const second = nextMirrorRecovery(2000, first.budget)!;
        expect(second.delayMs).toBe(7000);
        expect(nextMirrorRecovery(3000, second.budget)).toBeNull();
        expect(nextMirrorRecovery(121001, second.budget)?.budget.attempts).toBe(1);
    });
});
