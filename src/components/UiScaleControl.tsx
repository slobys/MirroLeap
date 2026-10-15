import { useEffect, useState } from 'react';
import { ZoomIn } from 'lucide-react';
import { useI18n } from '../i18n';

const LEVELS = [90, 100, 110, 125, 150] as const;
const STORAGE_KEY = 'scrcpy_ui_scale';

function loadScale(): number {
    try {
        const stored = Number(localStorage.getItem(STORAGE_KEY));
        return LEVELS.some(v => v === stored) ? stored : 100;
    } catch {
        return 100;
    }
}

/** Native webview zoom: scales formerly tiny labels as well as controls. */
export default function UiScaleControl() {
    const { t } = useI18n();
    const [scale, setScale] = useState(loadScale);

    useEffect(() => {
        document.documentElement.style.setProperty('zoom', scale + '%');
        try {
            localStorage.setItem(STORAGE_KEY, String(scale));
        } catch {
            // Browser storage may be disabled; the current session still works.
        }
        return () => { document.documentElement.style.removeProperty('zoom'); };
    }, [scale]);

    return (
        <label className="flex items-center gap-1.5 px-2 py-1 border border-zinc-800 rounded-lg bg-zinc-900/40 text-zinc-400"
               title={t('header.uiScale')}>
            <ZoomIn size={14} aria-hidden="true" />
            <span className="sr-only">{t('header.uiScale')}</span>
            <select
                aria-label={t('header.uiScale')}
                className="bg-transparent text-xs font-bold text-zinc-300 cursor-pointer outline-none"
                value={scale}
                onChange={event => setScale(Number(event.target.value))}
            >
                {LEVELS.map(level => <option key={level} value={level}>{level}%</option>)}
            </select>
        </label>
    );
}
