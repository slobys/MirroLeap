import {
    ArrowRight, Check, CircleHelp, Download, MonitorPlay, RefreshCw,
    Settings2, ShieldCheck, Smartphone, Volume2, VolumeX, Wifi, Youtube, Send,
} from 'lucide-react';
import type { PendingAdbDevice, ScrcpyConfig } from '../hooks/useScrcpy';
import {invoke} from '@tauri-apps/api/core';
import {useState, type MouseEvent} from 'react';
import VersionUpdateBadge from './VersionUpdateBadge';
import { mirrorVideoSummary, videoLoadWarning } from '../utils/mirrorPresets';

type Props = {
    locale: string;
    version: string;
    hasUpdate: boolean;
    checkingUpdate?: boolean;
    onOpenUpdate: () => void;
    devices: string[];
    pendingDevices: PendingAdbDevice[];
    deviceModels: Record<string, string>;
    deviceFriendlyNames: Record<string, string>;
    runningDevices: string[];
    activeDevice: string;
    onSelectDevice: (serial: string) => void;
    onRefresh: () => void;
    isRefreshing: boolean;
    config: ScrcpyConfig;
    onConfigChange: (config: ScrcpyConfig) => void;
    onResolutionChange: (size: string) => void;
    isSwitchingResolution: boolean;
    onStart: () => void;
    onStop: () => void;
    onOpenSettings: (tab?: 'devices' | 'mirror' | 'behavior' | 'appearance' | 'tools') => void;
    coreReady: boolean;
    onDownloadCore: () => void;
    isDownloading: boolean;
    downloadProgress: number;
};

const resolutions = [
    { value: '0', zh: '原始分辨率 · 可能延迟', en: 'Native size · Higher latency risk' },
    { value: '800', zh: '480p · 省流量', en: '480p · Low bandwidth' },
    { value: '1280', zh: '720p · 流畅', en: '720p · Smooth' },
    { value: '1920', zh: '1080P · 流畅推荐', en: '1080p · Recommended' },
    { value: '2560', zh: '2K · 高清', en: '2K · High definition' },
    { value: '3840', zh: '4K · 极致画质', en: '4K · Maximum detail' },
];

export default function CompactDashboard({
    locale, version, hasUpdate, checkingUpdate, onOpenUpdate,
    devices, pendingDevices, deviceModels, deviceFriendlyNames,
    runningDevices, activeDevice, onSelectDevice, onRefresh, isRefreshing,
    config, onConfigChange, onResolutionChange, isSwitchingResolution,
    onStart, onStop, onOpenSettings, coreReady, onDownloadCore,
    isDownloading, downloadProgress,
}: Props) {
    const zh = locale.startsWith('zh');
    const copy = (chinese: string, english: string) => zh ? chinese : english;
    const running = runningDevices.includes(activeDevice);
    const loadWarning = videoLoadWarning(config, locale);
    const selected = devices.includes(activeDevice);
    const deviceLabel = (serial: string) =>
        deviceFriendlyNames[serial] || deviceModels[serial] || serial;
    // Keep the old Stop button in place while the automatic stream handoff
    // briefly closes/reopens scrcpy; never display a misleading Start button.
    const visibleRunning = running || isSwitchingResolution;
    const [communityError,setCommunityError] = useState<string|null>(null);
    const openCommunity = async(event:MouseEvent<HTMLAnchorElement>, destination:'youtube'|'telegram')=>{
        // Ordinary web previews may use the external link directly. The
        // installed WebView2 has no default external-window permission,
        // so dispatch only an allowlisted destination to Rust.
        if (!('__TAURI_INTERNALS__' in window)) return;
        event.preventDefault();
        setCommunityError(null);
        try {
            await invoke('open_official_community_link',{destination});
        } catch(error) {
            setCommunityError(zh ? '无法打开外部浏览器，请检查默认浏览器设置。' :
                'Could not open the default browser.');
            console.warn('[MirroLeap] community browser open failed:',error);
        }
    };

    return (
        <main className="ml-home flex flex-col w-full h-full min-h-0">
            <header className="ml-home-header flex items-center justify-between gap-3">
                <div className="flex items-center gap-3 min-w-0">
                    <div className="ml-brand-icon shrink-0" aria-hidden="true">
                        <img src="/mirroleap-icon.svg" alt="" className="w-full h-full" />
                    </div>
                    <div className="min-w-0">
                        <h1 className="ml-wordmark text-[23px] leading-none tracking-tight font-extrabold">
                            Mirro<span>Leap</span>
                        </h1>
                        <p className="ml-muted text-[11px] mt-1.5 tracking-wide">
                            {copy('轻盈投屏，专注当下', 'Simply mirror. Stay focused.')}
                        </p>
                    </div>
                </div>
                <nav className="ml-community-links" aria-label={copy('官方交流频道','Community channels')}>
                    <a href="https://www.youtube.com/@naiyou" target="_blank" rel="noopener noreferrer"
                        onClick={event=>void openCommunity(event,'youtube')}
                        aria-label={copy('奶油 YouTube 频道','Naiyou YouTube channel')}
                        className="ml-community-link ml-community-youtube">
                        <Youtube size={19} aria-hidden="true"/>
                        <span>YouTube</span>
                    </a>
                    <a href="https://t.me/naiyous" target="_blank" rel="noopener noreferrer"
                        onClick={event=>void openCommunity(event,'telegram')}
                        aria-label={copy('奶油 Telegram 群组','Naiyou Telegram group')}
                        className="ml-community-link ml-community-telegram">
                        <Send size={17} aria-hidden="true"/>
                        <span>Telegram</span>
                    </a>
                </nav>
                {communityError && <span role="alert" className="text-xs text-red-500">{communityError}</span>}
                <div className="flex items-center gap-2 shrink-0">
                    <VersionUpdateBadge version={version} locale={locale}
                        hasUpdate={hasUpdate} checking={checkingUpdate}
                        onClick={onOpenUpdate}/>
                    <button type="button" className="ml-icon-button" aria-label={copy('打开设置', 'Open settings')}
                        title={copy('设置与高级功能', 'Settings and advanced features')}
                        onClick={() => onOpenSettings('mirror')}>
                        <Settings2 size={19} />
                    </button>
                </div>
            </header>

            <div className="ml-home-body w-full max-w-[1080px] mx-auto">
                <div className="ml-intro flex items-center justify-between gap-3">
                    <div>
                        <h2 className="text-xl font-bold tracking-tight">{copy('准备好投屏了吗？', 'Ready to mirror?')}</h2>
                        <p className="ml-muted mt-1 text-xs">
                            {copy('选择手机，一键开始。其他功能都在设置里。', 'Choose your phone and start in one click.')}
                        </p>
                    </div>
                    <span className={'ml-status-chip ' + (coreReady ? 'ml-ready' : 'ml-missing')} role="status">
                        <span className="ml-status-dot" />
                        {coreReady ? copy('引擎就绪', 'Engine ready') : copy('需要安装引擎', 'Engine missing')}
                    </span>
                </div>

                <div className="ml-dashboard-grid grid gap-4 items-stretch">
                    <section className="ml-card ml-device-card flex flex-col min-h-0" aria-label={copy('选择设备', 'Choose a device')}>
                        <div className="ml-card-head flex items-center justify-between">
                            <div className="flex items-center gap-2">
                                <Smartphone size={17} className="ml-primary" />
                                <h3 className="font-bold text-sm">{copy('我的设备', 'My devices')}</h3>
                                <span className="ml-count">{devices.length}</span>
                            </div>
                            <button type="button" className="ml-icon-button !w-9 !h-9" onClick={onRefresh}
                                aria-label={copy('刷新设备列表', 'Refresh devices')} disabled={isRefreshing}
                                title={copy('重新检测设备', 'Refresh')}>
                                <RefreshCw size={16} className={isRefreshing ? 'animate-spin' : ''} />
                            </button>
                        </div>
                        <div className="ml-device-list flex-1 min-h-0" role="group" aria-label={copy('已连接设备', 'Connected devices')}>
                            {devices.length === 0 ? (
                                <div className="ml-empty-state">
                                    <Smartphone size={30} strokeWidth={1.4} aria-hidden="true" />
                                    <strong className="mt-3 text-sm">{copy('还没有连接手机', 'No phone connected')}</strong>
                                    <p className="ml-muted text-xs text-center mt-1 leading-5">
                                        {copy('使用 USB 数据线，或在设置中配置无线连接。', 'Connect by USB or configure Wi-Fi in settings.')}
                                    </p>
                                </div>
                            ) : devices.map((serial) => {
                                const isSelected = activeDevice === serial;
                                const isRunning = runningDevices.includes(serial);
                                const wireless = serial.includes(':') || serial.startsWith('adb-');
                                return (
                                    <button key={serial} type="button"
                                        className={'ml-device-row ' + (isSelected ? 'ml-device-selected' : '')}
                                        aria-pressed={isSelected}
                                        onClick={() => onSelectDevice(serial)}>
                                        <span className="ml-device-symbol"><Smartphone size={20} /></span>
                                        <span className="flex-1 min-w-0 text-left">
                                            <strong className="block truncate text-[13px] font-bold">{deviceLabel(serial)}</strong>
                                            <span className="block ml-muted truncate text-[11px] mt-0.5">
                                                {wireless ? 'Wi-Fi' : 'USB'} · {isRunning ? copy('投屏中', 'Mirroring') : copy('已连接', 'Connected')}
                                            </span>
                                        </span>
                                        {isSelected ? <Check className="ml-primary shrink-0" size={17} aria-hidden="true" /> :
                                            <span className="ml-device-dot" aria-hidden="true" />}
                                    </button>
                                );
                            })}
                            {pendingDevices.length > 0 && (
                                <p className="ml-muted text-[11px] px-2 mt-3" role="note">
                                    {copy('部分设备未授权或离线，请在设置中查看。', 'Some devices need authorization or are offline.')}
                                </p>
                            )}
                        </div>
                        <button className="ml-secondary-action mt-3 w-full flex items-center justify-center gap-2"
                            type="button" onClick={() => onOpenSettings('devices')}>
                            <Wifi size={15} /> {copy('无线连接 / 配对设备', 'Wi-Fi connection / Pair device')}
                            <ArrowRight size={14} />
                        </button>
                    </section>

                    <section className="ml-card ml-mirror-card flex flex-col" aria-label={copy('投屏控制', 'Mirror controls')}>
                        <div className="ml-card-head flex items-center gap-2">
                            <MonitorPlay size={18} className="ml-primary" />
                            <h3 className="font-bold text-sm">{copy('屏幕投屏', 'Screen mirroring')}</h3>
                        </div>
                        <div className="ml-hero flex items-center gap-3 mt-3">
                            <div className="ml-hero-icon"><Smartphone size={27} strokeWidth={1.7}/></div>
                            <div className="min-w-0 flex-1">
                                <div className="font-semibold text-[14px] truncate">
                                    {selected ? deviceLabel(activeDevice) : copy('请选择一台手机', 'Select a device')}
                                </div>
                                <div className="ml-muted text-xs mt-1">
                                    {running ? copy('投屏正在运行', 'Your screen is being mirrored') :
                                        selected ? copy('已连接，可以开始投屏', 'Connected and ready') :
                                        copy('连接后即可开始投屏', 'Connect a phone to get started')}
                                </div>
                            </div>
                            <span className={'ml-running-light ' + (running ? 'ml-on' : '')} aria-hidden="true" />
                        </div>

                        <div className="ml-field mt-3">
                            <label htmlFor="ml-resolution" className="ml-field-title">{copy('分辨率', 'Resolution')}</label>
                            <select id="ml-resolution" value={config.res || '0'}
                                disabled={isSwitchingResolution}
                                onChange={(event) => onResolutionChange(event.target.value)}
                                className="ml-select">
                                {resolutions.map((option) => <option key={option.value} value={option.value}>
                                    {zh ? option.zh : option.en}
                                </option>)}
                                {!resolutions.some(({value}) => value === config.res) && config.res !== '0' &&
                                    <option value={config.res}>{config.res}</option>}
                            </select>
                            <p className="ml-muted mt-2 text-xs leading-5" role="note" data-testid="home-video-summary">
                                {copy('当前参数：', 'Current settings: ')}{mirrorVideoSummary(config, locale)}
                            </p>
                            {visibleRunning && (
                                <p className="ml-muted mt-1 text-[11px] leading-5" role="note">
                                    {isSwitchingResolution
                                        ? copy('正在自动切换视频流，请稍候…', 'Applying new video stream…')
                                        : copy('切换分辨率将自动匹配画质参数并短暂重启视频，无需手动停止。',
                                            'Changing resolution applies matching video settings automatically (brief restart).')}
                                </p>
                            )}
                            {loadWarning && (
                                <p className="mt-1 text-[11px] leading-5 text-amber-600 dark:text-amber-400" role="note">
                                    {loadWarning}
                                </p>
                            )}
                        </div>
                        <div className="ml-quick-toggles grid grid-cols-2 gap-3 mt-3">
                            <button className={'ml-toggle-card ' + (config.deviceFrame ? 'ml-enabled' : '')}
                                type="button" aria-pressed={Boolean(config.deviceFrame)}
                                onClick={() => onConfigChange({...config, deviceFrame: !config.deviceFrame})}>
                                <ShieldCheck size={17}/>
                                <span>{copy('真机边框', 'Phone frame')}</span>
                                <span className="ml-switch" aria-hidden="true"/>
                            </button>
                            <button className={'ml-toggle-card ' + (config.audioEnabled !== false ? 'ml-enabled' : '')}
                                type="button" aria-pressed={config.audioEnabled !== false}
                                onClick={() => onConfigChange({...config, audioEnabled: config.audioEnabled === false})}>
                                {config.audioEnabled === false ? <VolumeX size={17}/> : <Volume2 size={17}/>}
                                <span>{copy('声音', 'Audio')}</span>
                                <span className="ml-switch" aria-hidden="true"/>
                            </button>
                        </div>
                        <div className="ml-card-spacer" aria-hidden="true" />
                        {!coreReady ? (
                            <button type="button" className="ml-primary-action flex items-center justify-center gap-2 mt-5"
                                disabled={isDownloading} onClick={onDownloadCore}>
                                <Download size={19}/>
                                {isDownloading ? copy(`正在安装 ${downloadProgress}%`, `Installing ${downloadProgress}%`)
                                    : copy('安装投屏引擎', 'Install mirroring engine')}
                            </button>
                        ) : visibleRunning ? (
                            <button type="button" onClick={onStop} disabled={isSwitchingResolution}
                                className="ml-stop-action flex items-center justify-center gap-2 mt-5 disabled:opacity-60">
                                <span className="w-3 h-3 bg-current rounded-[3px]" aria-hidden="true"/>
                                {isSwitchingResolution
                                    ? copy('正在切换画质…', 'Applying video quality…')
                                    : copy('停止投屏', 'Stop mirroring')}
                            </button>
                        ) : (
                            <button type="button" onClick={onStart} disabled={!selected || isSwitchingResolution}
                                className="ml-primary-action flex items-center justify-center gap-2 mt-5">
                                <MonitorPlay size={20}/>
                                {copy('开始投屏', 'Start mirroring')}
                                <ArrowRight size={16}/>
                            </button>
                        )}
                        {isSwitchingResolution && (
                            <p className="ml-muted text-xs mt-2 text-center" role="status">
                                {copy('正在自动应用画质，Wi-Fi 配对保留，请稍候…',
                                    'Applying video settings; Wi-Fi pairing stays connected…')}
                            </p>
                        )}
                    </section>
                </div>
                <footer className="ml-home-footer flex items-center justify-between gap-3 flex-wrap">
                    <span className="ml-muted text-[11px] flex items-center gap-1.5">
                        <ShieldCheck size={13} /> {copy('基于开源 scrcpy · 本地投屏', 'Powered by open-source scrcpy')}
                    </span>
                    <button type="button" className="ml-footer-link flex items-center gap-1"
                        onClick={() => onOpenSettings('behavior')}>
                        <CircleHelp size={14} /> {copy('更多功能与快捷键', 'More features & shortcuts')}
                        <ArrowRight size={13}/>
                    </button>
                </footer>
            </div>
        </main>
    );
}
