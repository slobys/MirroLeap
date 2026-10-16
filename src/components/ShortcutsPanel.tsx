import { Maximize, Home, ChevronLeft, List, Power, RotateCw, Clipboard, MonitorOff, Keyboard } from 'lucide-react';
import { useI18n } from '../i18n';

export default function ShortcutsPanel() {
    const { t } = useI18n();
    const shortcuts = [
        { label: t('shortcuts.full'), key: "F", icon: Maximize },
        { label: t('shortcuts.home'), key: "H", icon: Home },
        { label: t('shortcuts.back'), key: "B", icon: ChevronLeft },
        { label: t('shortcuts.recents'), key: "S", icon: List },
        { label: t('shortcuts.power'), key: "P", icon: Power },
        { label: t('shortcuts.rotate'), key: "R", icon: RotateCw },
        { label: t('shortcuts.paste'), key: "V", icon: Clipboard },
        { label: t('shortcuts.off'), key: "O", icon: MonitorOff },
    ];

    return (
        <div className="ml-card ml-shortcuts-panel space-y-3">
            <div className="flex items-center gap-2 border-b border-zinc-800/50 pb-1.5 mb-1">
                <Keyboard size={12} className="text-zinc-500" />
                <h2 className="text-[17px] font-bold">{t('shortcuts.title')}</h2>
            </div>
            <div className="ml-shortcut-grid grid grid-cols-2 sm:grid-cols-4 xl:grid-cols-8 gap-3">
                {shortcuts.map(s => (
                    <div key={s.key} className="group relative flex flex-col items-center justify-center bg-zinc-950/30 p-1.5 rounded-lg border border-transparent hover:border-zinc-700 transition-all cursor-help">
                        {/* Tooltip on Hover */}
                        <s.icon size={17} className="text-zinc-500 group-hover:text-primary transition-colors mb-1" />
                        <kbd className="min-w-[24px] h-6 flex items-center justify-center text-xs font-bold bg-zinc-800/50 text-zinc-400 group-hover:text-white group-hover:bg-primary/20 px-1 rounded border border-zinc-800 group-hover:border-primary/50 transition-all">
                            {s.key}
                        </kbd>
                        <span className="ml-shortcut-title">{s.label}</span>
                    </div>
                ))}
            </div>
        </div>
    );
}
