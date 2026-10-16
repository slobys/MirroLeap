import { useEffect, useRef, useState } from 'react';
import { Download, FolderOpen, RefreshCcw, Palette, HelpCircle, X, ExternalLink, Languages, ChevronDown, Sun, Moon, Monitor } from 'lucide-react';
import { SUPPORTED_LOCALES, useI18n, type Locale } from '../i18n';
import UiScaleControl from './UiScaleControl';

interface HeaderProps {
    onThemeChange: (theme: string) => void;
    currentTheme: string;
    colorMode: 'light' | 'dark' | 'system';
    onColorModeChange: (mode: 'light' | 'dark' | 'system') => void;
    binaryStatus: { found: boolean, message: string };
    onDownload: () => void;
    onSetPath: () => void;
    onResetPath: () => void;
    isDownloading: boolean;
    downloadProgress: number;
    version: string;
}

export default function Header({ onThemeChange, currentTheme, colorMode, onColorModeChange, binaryStatus, onDownload, onSetPath, onResetPath, isDownloading, downloadProgress, version }: HeaderProps) {
    const { t, locale, setLocale, translations } = useI18n();
    const zh = locale.startsWith('zh');
    const [showHelp, setShowHelp] = useState(false);
    const [showLangMenu, setShowLangMenu] = useState(false);
    const langMenuRef = useRef<HTMLDivElement>(null);

    useEffect(() => {
        if (!showLangMenu) return;
        const onClick = (event: MouseEvent) => {
            if (langMenuRef.current && !langMenuRef.current.contains(event.target as Node)) setShowLangMenu(false);
        };
        document.addEventListener('mousedown', onClick);
        return () => document.removeEventListener('mousedown', onClick);
    }, [showLangMenu]);

    return <div className="ml-appearance-controls">
        <section className="ml-card ml-appearance-preferences">
            <div className="ml-settings-group-head">
                <Palette size={19} className="ml-primary"/>
                <div>
                    <h2>{zh ? '界面与个性化' : 'Appearance & personalization'}</h2>
                    <p className="ml-muted">{zh ? '选择舒适的显示方式' : 'Choose a comfortable look'}</p>
                </div>
            </div>

            <div className="ml-preference-field">
                <span className="ml-preference-label">{t('header.themeLabel')}</span>
                <div className="ml-theme-swatches">
                    {[
                        {id:'ultraviolet',color:'#8b5cf6',label:t('header.themes.ultraviolet')},
                        {id:'astro',color:'#3b82f6',label:t('header.themes.astro')},
                        {id:'carbon',color:'#ffffff',label:t('header.themes.carbon')},
                        {id:'emerald',color:'#10b981',label:t('header.themes.emerald')},
                        {id:'bloodmoon',color:'#ef4444',label:t('header.themes.bloodmoon')},
                    ].map(theme => (
                        <button type="button" key={theme.id} title={theme.label}
                            aria-label={theme.label} aria-pressed={currentTheme === theme.id}
                            className={'ml-theme-swatch ' + (currentTheme === theme.id ? 'ml-selected' : '')}
                            style={{backgroundColor:theme.color}}
                            onClick={()=>onThemeChange(theme.id)}
                        />
                    ))}
                </div>
            </div>

            <div className="ml-preference-field">
                <span className="ml-preference-label">{zh ? '显示模式' : 'Color mode'}</span>
                <div className="ml-preference-segments">
                    {([
                        {id:'light' as const,Icon:Sun,label:t('header.colorModes.light')},
                        {id:'dark' as const,Icon:Moon,label:t('header.colorModes.dark')},
                        {id:'system' as const,Icon:Monitor,label:t('header.colorModes.system')},
                    ]).map(({id,Icon,label})=>(
                        <button key={id} type="button" title={label} aria-label={label} aria-pressed={colorMode===id}
                            onClick={()=>onColorModeChange(id)}
                            className={'ml-preference-segment ' + (colorMode===id ? 'ml-selected' : '')}>
                            <Icon size={17}/><span>{label}</span>
                        </button>
                    ))}
                </div>
            </div>

            <div className="ml-preference-field">
                <span className="ml-preference-label">{zh ? '界面缩放' : 'Interface scale'}</span>
                <div className="ml-appearance-scale"><UiScaleControl/></div>
            </div>

            <div className="ml-preference-field">
                <span className="ml-preference-label">{t('header.languageLabel')}</span>
                <div className="relative" ref={langMenuRef}>
                    <button type="button" aria-expanded={showLangMenu} aria-haspopup="listbox"
                        onClick={()=>setShowLangMenu(value=>!value)}
                        className="ml-secondary-action ml-language-select flex items-center justify-between gap-3 px-4">
                        <span className="inline-flex items-center gap-2"><Languages size={17}/>{translations.languages[locale]}</span>
                        <ChevronDown size={15} className={showLangMenu ? 'rotate-180' : ''}/>
                    </button>
                    {showLangMenu && <div role="listbox"
                        className="absolute top-full left-0 right-0 z-[120] mt-2 rounded-xl border border-[var(--border-base)] bg-[var(--bg-surface)] p-1 shadow-xl max-h-56 overflow-y-auto">
                        {SUPPORTED_LOCALES.map((item:Locale)=>(
                            <button type="button" key={item} role="option" aria-selected={locale===item}
                                onClick={()=>{setLocale(item);setShowLangMenu(false);}}
                                className={'ml-language-option ' + (locale===item ? 'ml-language-current' : '')}>
                                {translations.languages[item]}
                            </button>
                        ))}
                    </div>}
                </div>
            </div>
        </section>

        <section className="ml-card ml-appearance-engine">
            <div className="ml-settings-group-head">
                <Monitor size={19} className="ml-primary"/>
                <div className="flex-1 min-w-0">
                    <h2>{t('header.scrcpyEngine')}</h2>
                    <p className="ml-muted">{zh ? '本地投屏引擎' : 'Local mirroring engine'}</p>
                </div>
                <span className={'ml-engine-badge ' + (binaryStatus.found ? 'ml-engine-ready' : 'ml-engine-unavailable')}>
                    {binaryStatus.found ? t('header.scrcpyReady') : (zh ? '需要设置' : 'Setup needed')}
                </span>
            </div>
            <div className="ml-engine-summary">
                <span className="ml-muted">{zh ? '当前状态' : 'Status'}</span>
                <strong className={binaryStatus.found ? 'text-emerald-500' : 'text-amber-500'}>
                    {isDownloading ? t('header.syncingComponents',{progress:downloadProgress}) :
                        binaryStatus.found ? t('header.scrcpyReady') : binaryStatus.message}
                </strong>
                {isDownloading && <div className="ml-engine-progress"><div style={{width:downloadProgress+'%'}}/></div>}
            </div>
            <div className="ml-engine-actions">
                {!binaryStatus.found && !isDownloading &&
                    <button type="button" onClick={onDownload} className="ml-secondary-action inline-flex items-center gap-2 px-4">
                        <Download size={16}/>{t('header.installCore')}
                    </button>}
                <button type="button" onClick={onSetPath} className="ml-secondary-action inline-flex items-center gap-2 px-4">
                    <FolderOpen size={16}/>{t('header.selectFolder')}
                </button>
                <button type="button" onClick={onResetPath} className="ml-secondary-action inline-flex items-center gap-2 px-4">
                    <RefreshCcw size={16}/>{t('header.resetPath')}
                </button>
                {!binaryStatus.found &&
                    <button type="button" onClick={()=>setShowHelp(true)}
                        className="ml-secondary-action inline-flex items-center gap-2 px-4">
                        <HelpCircle size={16}/>{t('header.setupHelp')}
                    </button>}
            </div>
            <p className="ml-muted ml-engine-footnote">MirroLeap v{version} · {zh ? '保留现有引擎路径与配置' : 'Existing engine settings are preserved'}</p>
        </section>
        {showHelp && <div className="fixed inset-0 z-[100] flex items-center justify-center bg-black/60 backdrop-blur-sm p-4">
            <div role="dialog" aria-modal="true" aria-label={t('header.manualSetupGuide')}
                className="glass max-w-md w-full p-6 rounded-2xl border border-zinc-800 shadow-2xl bg-zinc-950/90">
                <div className="flex justify-between items-center mb-4">
                    <h3 className="text-base font-bold text-primary flex items-center gap-2">
                        <HelpCircle size={18}/>{t('header.manualSetupGuide')}
                    </h3>
                    <button type="button" onClick={()=>setShowHelp(false)} aria-label={zh ? '关闭' : 'Close'}>
                        <X size={20}/>
                    </button>
                </div>
                <p className="text-sm">{t('header.manualSetupIntro')}</p>
                <ol className="list-decimal list-inside space-y-3 text-sm mt-4">
                    <li>{t('header.manualStep1')}
                        <a href="https://github.com/Genymobile/scrcpy/releases/latest" target="_blank"
                            rel="noopener noreferrer" className="ml-primary inline-flex items-center gap-1">
                            {t('header.manualStep1Link')}<ExternalLink size={14}/>
                        </a>
                    </li>
                    <li>{t('header.manualStep2')}</li>
                    <li>{t('header.manualStep3Before')} {t('header.manualStep3Browse')} {t('header.manualStep3After')}</li>
                    <li>{t('header.manualStep4Before')} <code>{t('header.manualStep4Executable')}</code> {t('header.manualStep4After')}</li>
                </ol>
                <p className="ml-muted text-sm mt-4">{t('header.manualNote')}</p>
                <button type="button" onClick={()=>setShowHelp(false)}
                    className="mt-5 w-full bg-primary text-on-primary rounded-xl p-3">{t('header.gotIt')}</button>
            </div>
        </div>}
    </div>;
}
