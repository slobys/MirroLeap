import { AlertCircle, CheckCircle2, LoaderCircle, ShieldCheck, Wifi } from 'lucide-react';
import { useI18n } from '../i18n';
import type { WirelessFeedback } from '../utils/wirelessCenter';
import { safeDiagnosisCode, safeSameHostAlternatives } from '../utils/wirelessCenter';

type Props = {
    feedback: WirelessFeedback;
    onSelectPort: (address: string) => void;
};

/** One accessible, localized connection result shared by manual connect,
 * diagnostics, USB→Wi-Fi and TLS pairing. Never claims that port_open means
 * ADB authorization, or that pairing alone established a device session.
 */
export default function WirelessConnectionStatus({ feedback, onSelectPort }: Props) {
    const { t } = useI18n();
    const busy = feedback.phase === 'connecting' || feedback.phase === 'diagnosing';
    const success = feedback.phase === 'connected' || feedback.phase === 'paired';
    const alternatives = safeSameHostAlternatives(feedback.diagnosis, feedback.address);
    const current = safeDiagnosisCode(feedback.diagnosis);

    return <section role="status" aria-live="polite" aria-label={t('sidebar.wirelessCenter.title')}
        className="rounded-lg border border-primary/30 bg-primary/5 p-3 space-y-2">
        <div className="flex items-center gap-2 text-xs font-semibold text-zinc-200">
            {busy ? <LoaderCircle size={15} className="animate-spin text-primary"/> :
                success ? <CheckCircle2 size={15} className="text-emerald-400"/> :
                <AlertCircle size={15} className="text-amber-400"/>}
            <span>{t('sidebar.wirelessCenter.' + feedback.phase)}</span>
        </div>
        <p className="text-[11px] text-zinc-400 break-all">{feedback.address}</p>
        {feedback.diagnosis && <p className="text-xs text-zinc-300 leading-relaxed">
            {t('sidebar.diagnostics.' + current)}
        </p>}
        {feedback.phase === 'connected' && <p className="text-[11px] text-zinc-400">
            {t('sidebar.wirelessCenter.connectedHelp')}
        </p>}
        {feedback.phase === 'paired' && <p className="text-[11px] text-zinc-400">
            {t('sidebar.wirelessCenter.pairedHelp')}
        </p>}
        {feedback.phase === 'failed' && <p className="text-[11px] text-zinc-400">
            {t('sidebar.wirelessCenter.failedHelp')}
        </p>}
        {alternatives.length > 0 && <div className="space-y-2">
            {alternatives.map(port => <button key={port} type="button"
                onClick={() => onSelectPort(port)}
                className="w-full rounded-md border border-primary/30 px-2 py-2 text-left text-xs text-primary hover:bg-primary/10 break-all">
                <Wifi size={13} className="inline mr-1.5"/>
                {t('sidebar.useDiscoveredPort')}: {port}
            </button>)}
        </div>}
        {feedback.phase === 'needs_action' && current === 'unauthorized' &&
            <p className="flex items-center gap-1 text-[11px] text-zinc-400">
                <ShieldCheck size={13}/>{t('sidebar.wirelessCenter.manualPort')}
            </p>}
    </section>;
}
