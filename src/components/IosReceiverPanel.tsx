import { useEffect, useState, type MouseEvent } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { Airplay, CheckCircle2, Download, ExternalLink, Expand, LayoutPanelLeft, LoaderCircle, ShieldAlert, Square, X } from 'lucide-react';

export type IosAirplayStatus = {
    supported: boolean;
    installed: boolean;
    running: boolean;
    installing: boolean;
    receiverName: string;
    engineVersion: string;
    compactMode: boolean;
};

type Props = { locale: string; onClose: () => void };

export default function IosReceiverPanel({locale, onClose}: Props) {
    const zh = locale.startsWith('zh');
    const copy = (cn: string, en: string) => zh ? cn : en;
    const [status,setStatus] = useState<IosAirplayStatus | null>(null);
    const [busy,setBusy] = useState<'install'|'start'|'stop'|'layout'|'view'|null>(null);
    const [error,setError] = useState<string | null>(null);
    const [viewFeedback,setViewFeedback] = useState<string | null>(null);

    useEffect(() => {
        let active=true;
        const refresh=async()=>{
            try {
                const next=await invoke<IosAirplayStatus>('ios_airplay_status');
                if(active) setStatus(next);
            } catch (e) {
                if(active) setError(String(e));
            }
        };
        void refresh();
        // Receiver process running ≠ iPhone connected. Poll process liveness,
        // not speculative AirPlay discovery or a fake connected state.
        const timer=setInterval(()=>void refresh(),4000);
        return ()=>{active=false;clearInterval(timer);};
    },[]);

    const run=async(action:'install'|'start'|'stop')=>{
        if(busy) return;
        setBusy(action);setError(null);
        try {
            const next=await invoke<IosAirplayStatus>('ios_airplay_'+action);
            setStatus(next);
        } catch(e) {
            setError(String(e));
            try {setStatus(await invoke<IosAirplayStatus>('ios_airplay_status'));}
            catch { /* Keep the action error visible. */ }
        } finally {setBusy(null);}
    };

    const setCompact=async(compact:boolean)=>{
        if(busy || status?.running || !status?.supported) return;
        setBusy('layout');setError(null);setViewFeedback(null);
        try {
            setStatus(await invoke<IosAirplayStatus>('ios_airplay_set_compact_mode',{compact}));
        } catch(error) {setError(String(error));}
        finally {setBusy(null);}
    };
    const viewAction=async(action:'toggle_clean'|'fullscreen')=>{
        if(busy || !status?.running) return;
        setBusy('view');setError(null);setViewFeedback(null);
        try {
            setStatus(await invoke<IosAirplayStatus>('ios_airplay_window_action',{action}));
            setViewFeedback(action==='toggle_clean'?
                copy('已向接收窗口发送 H 切换请求；若未生效，请点击接收窗口后按 H。',
                     'Sent H request to the receiver. If needed, click its window and press H.') :
                copy('已发送 F11 全屏切换请求；若未生效，请点击接收窗口后按 F11。',
                     'Sent F11 fullscreen request. If needed, click its window and press F11.'));
        } catch(error) {setError(String(error));}
        finally {setBusy(null);}
    };

    const openSource=async(event:MouseEvent<HTMLAnchorElement>)=>{
        if(!('__TAURI_INTERNALS__' in window)) return;
        event.preventDefault();
        try {await invoke('ios_airplay_open_source');}
        catch(e){setError(String(e));}
    };
    const working=busy!==null || status?.installing===true;

    return (
        <div role="presentation" className="fixed inset-0 z-[105] flex items-center justify-center p-3 sm:p-6"
            style={{background:'rgba(0,0,0,.72)'}}>
            <section role="dialog" aria-modal="true" aria-label={copy('iOS AirPlay 投屏中心','iOS AirPlay receiver')}
                className="w-full max-w-[580px] max-h-[min(88vh,740px)] overflow-y-auto rounded-2xl border border-zinc-500/30 shadow-2xl p-5 sm:p-7"
                style={{background:'var(--bg-base)',color:'var(--text-base)'}}>
                <div className="flex items-start gap-3 justify-between">
                    <div className="flex gap-3 items-center min-w-0">
                        <span className="p-3 rounded-xl bg-emerald-500/15 text-emerald-400"><Airplay size={25}/></span>
                        <div>
                            <h2 className="font-bold text-lg">{copy('iPhone / iPad 无线投屏','iPhone / iPad screen mirroring')}</h2>
                            <p className="ml-muted text-xs mt-1">{copy('MirroLeap · iOS AirPlay 接收中心','MirroLeap · iOS AirPlay receiver')}</p>
                        </div>
                    </div>
                    <button type="button" onClick={onClose} className="ml-icon-button shrink-0"
                        aria-label={copy('关闭 iOS 面板','Close iOS panel')}><X size={19}/></button>
                </div>
                <div className="rounded-xl border border-zinc-500/20 p-4 mt-5">
                    <div className="flex items-center gap-2">
                        {busy && <LoaderCircle className="animate-spin text-emerald-400" size={16}/>}
                        {!busy && status?.running && <CheckCircle2 className="text-emerald-400" size={17}/>}
                        <strong className="text-sm" role="status">
                            {!status ? copy('正在检测接收组件…','Checking receiver…')
                                : !status.supported ? copy('仅支持 Windows x64','Windows x64 required')
                                : busy==='install' ? copy('正在安装并验证接收组件…','Installing and verifying receiver…')
                                : !status.installed ? copy('尚未安装 AirPlay 接收组件','AirPlay receiver not installed')
                                : status.running ? copy('接收器已启动 · 等待 iPhone 连接','Receiver running · awaiting iPhone')
                                : copy('接收组件已安装 · 尚未启动','Receiver installed · not running')}
                        </strong>
                    </div>
                    <p className="ml-muted text-xs mt-2 leading-5">
                        {copy('iOS 不使用 Android ADB。AirPlay 接收器单独打开一个 Windows 窗口，不会改变现有 Android 投屏。',
                            'iOS uses AirPlay instead of Android ADB. The receiver opens its own Windows window and does not change Android mirroring.')}
                    </p>
                    {!status?.installed && status?.supported && (
                        <p className="text-xs mt-2 leading-5">
                            {copy('首次安装将从 GitHub 下载约 12 MB 的开源 GPL-3.0 接收组件，验证 SHA-256 后保存在当前 Windows 用户目录；不会自动下载。',
                                'First-time setup downloads a ~12 MB GPL-3.0 receiver from GitHub, verifies its SHA-256 and installs to your user profile. No automatic download.')}
                        </p>
                    )}
                    <div className="mt-4 flex gap-2 flex-wrap">
                        {status?.supported && !status.installed && (
                            <button disabled={working} type="button" className="ml-primary-action !w-auto px-4 flex items-center justify-center gap-2 disabled:opacity-50"
                                onClick={()=>void run('install')}><Download size={16}/>
                                {copy('下载安装 AirPlay 组件','Install AirPlay component')}</button>
                        )}
                        {status?.supported && status.installed && !status.running && (
                            <button disabled={working} type="button" className="ml-primary-action !w-auto px-4 flex items-center gap-2 disabled:opacity-50"
                                onClick={()=>void run('start')}><Airplay size={17}/>
                                {copy('启动 iOS 接收器','Start iOS receiver')}</button>
                        )}
                        {status?.supported && status.running && (
                            <button disabled={working} type="button" className="ml-stop-action !w-auto px-4 flex items-center gap-2 disabled:opacity-50"
                                onClick={()=>void run('stop')}><Square size={15}/>
                                {copy('停止 iOS 接收器','Stop receiver')}</button>
                        )}
                    </div>
                    {status?.supported && status.installed && (
                        <div className="mt-4 rounded-xl border border-zinc-500/20 p-3">
                            <p className="font-semibold text-xs mb-2">{copy('投屏窗口风格','Receiver window layout')}</p>
                            <div role="group" aria-label={copy('iOS 投屏窗口布局','iOS mirroring window layout')}
                                className="grid grid-cols-2 gap-2">
                                <button type="button" disabled={working||status.running}
                                    aria-pressed={status.compactMode}
                                    onClick={()=>void setCompact(true)}
                                    className={`rounded-lg border px-3 py-2 text-xs flex items-center justify-center gap-1.5 disabled:opacity-60 ${status.compactMode?'border-emerald-500 bg-emerald-500/15':'border-zinc-500/30'}`}>
                                    <Airplay size={15}/>{copy('简洁投屏（推荐）','Clean view (recommended)')}
                                </button>
                                <button type="button" disabled={working||status.running}
                                    aria-pressed={!status.compactMode}
                                    onClick={()=>void setCompact(false)}
                                    className={`rounded-lg border px-3 py-2 text-xs flex items-center justify-center gap-1.5 disabled:opacity-60 ${!status.compactMode?'border-emerald-500 bg-emerald-500/15':'border-zinc-500/30'}`}>
                                    <LayoutPanelLeft size={15}/>{copy('原版控制界面','Original UI')}
                                </button>
                            </div>
                            <p className="ml-muted text-xs mt-2 leading-5">
                                {copy('简洁模式启动后只显示投屏画面，隐藏英文侧栏与大块留白，窗口可拖动、缩放并跟随 iPad 旋转。此选项在下次启动接收器时生效。',
                                      'Clean view removes the sidebar and whitespace, supports dragging/resizing and follows iPad rotation. The setting applies on the next receiver start.')}
                            </p>
                            {status.running && <div className="flex flex-wrap gap-2 mt-3">
                                <button type="button" disabled={working} onClick={()=>void viewAction('toggle_clean')}
                                    className="ml-secondary-action !min-h-9 px-3 inline-flex items-center gap-2 text-xs">
                                    <LayoutPanelLeft size={15}/>{copy('切换简洁/原界面（H）','Toggle clean/original (H)')}
                                </button>
                                <button type="button" disabled={working} onClick={()=>void viewAction('fullscreen')}
                                    className="ml-secondary-action !min-h-9 px-3 inline-flex items-center gap-2 text-xs">
                                    <Expand size={15}/>{copy('全屏/退出（F11）','Toggle fullscreen (F11)')}
                                </button>
                            </div>}
                            {viewFeedback && <p role="status" className="text-xs mt-2 leading-5">{viewFeedback}</p>}
                        </div>
                    )}
                    {error && <p className="mt-3 text-xs text-red-400 whitespace-pre-wrap break-words" role="alert">{error}</p>}
                </div>

                <div className="mt-5">
                    <h3 className="font-semibold text-sm">{copy('在 iPhone / iPad 上连接','Connect from iPhone / iPad')}</h3>
                    <ol className="mt-2 space-y-2 text-[13px] leading-5 list-decimal pl-5">
                        <li>{copy('确保电脑与 iPhone 在同一 Wi-Fi / 局域网，关闭访客网络隔离。',
                            'Connect iPhone and PC to the same local network; disable guest isolation.')}</li>
                        <li>{copy('点击启动接收器，若 Windows 防火墙询问，允许可信的“专用网络”访问。',
                            'Start the receiver; allow access on trusted Private networks if Windows Firewall asks.')}</li>
                        <li>{copy('在 iPhone 打开控制中心 → 屏幕镜像，选择“AirPlay-Windows”（可在接收器 OPTIONS 修改名称）。',
                            'On iPhone open Control Center → Screen Mirroring → AirPlay-Windows (name can be changed in receiver OPTIONS).')}</li>
                        <li>{copy('如出现配对提示，按接收器窗口提示完成；停止时在 iPhone 选择“停止镜像”。',
                            'Follow any on-screen pairing prompt; choose Stop Mirroring on iPhone to disconnect.')}</li>
                    </ol>
                </div>

                <div className="mt-5 flex items-start gap-2 text-xs ml-muted">
                    <ShieldAlert size={16} className="shrink-0 mt-0.5"/>
                    <p className="leading-5">{copy(
                        'iOS 接收阶段使用可缩放的独立窗口，简洁模式不显示第三方接收器侧栏；不支持 ADB 触控控制、Netflix/Apple TV+ 等视频 DRM 绕过。接收器运行不代表 iPhone 已连接。',
                        'iOS receiver phase 1: wireless mirror in a separate window with its own audio. No ADB remote control or bypass of protected video DRM. A running receiver does not mean an iPhone is connected.'
                    )}</p>
                </div>
                <div className="mt-4 pt-3 border-t border-zinc-500/20 flex items-center justify-between gap-2 flex-wrap">
                    <span className="ml-muted text-xs">{copy('开源引擎：AirPlay-Windows v0.4.1 · GPL-3.0','Engine: AirPlay-Windows v0.4.1 · GPL-3.0')}</span>
                    <a href="https://github.com/moieric11/AirPlay-Windows" target="_blank" rel="noopener noreferrer"
                        className="ml-footer-link flex items-center gap-1 text-xs"
                        onClick={event=>void openSource(event)}>{copy('源码与许可证','Source & license')}<ExternalLink size={14}/></a>
                </div>
            </section>
        </div>
    );
}
