import {useEffect, useState} from 'react';
import {Download, ExternalLink, RefreshCw, ShieldCheck, X} from 'lucide-react';
import {invoke} from '@tauri-apps/api/core';
import {listen} from '@tauri-apps/api/event';
import type {AppUpdateInfo} from '../utils/appUpdate';
import {useI18n} from '../i18n';

type Props = {
    release: AppUpdateInfo;
    installKind: 'installed' | 'portable' | 'other';
    running: boolean;
    onDismiss: () => void;
};

/** Startup check is read-only. Only this button can invoke the native signed
 * installer. No background download, forced relaunch or unsigned installer.
 */
export default function StartupUpdateNotice({
    release,installKind,running,onDismiss,
}:Props) {
    const {locale}=useI18n();
    const zh=locale.startsWith('zh');
    const [busy,setBusy]=useState(false);
    const [message,setMessage]=useState<string|null>(null);
    const [error,setError]=useState<string|null>(null);
    const [progress,setProgress]=useState<number|null>(null);
    const isNsisInstall=installKind==='installed';
    const canInstall=isNsisInstall && !running;

    useEffect(()=>{
        let active=true;
        const unlisten=listen<{percent?:number}>('mirroleap-app-update-progress',({payload})=>{
            if(active && typeof payload?.percent==='number' && Number.isFinite(payload.percent)) {
                setProgress(Math.max(0,Math.min(100,Math.round(payload.percent))));
            }
        });
        return ()=>{
            active=false;
            void unlisten.then(dispose=>dispose());
        };
    },[]);

    const proceed=async()=>{
        if(busy) return;
        if(running && installKind==='installed') {
            setError(zh?'请先停止手机投屏，再点击更新。':
                'Stop the active phone mirror before installing.');
            return;
        }
        setBusy(true);
        setError(null);
        setMessage(canInstall ?
            (zh?'正在核对官方版本及签名，随后下载安装…':'Checking release signature and downloading…') :
            (zh?'正在打开官方下载页面…':'Opening the official download page…'));
        try {
            if(canInstall) {
                await invoke('install_signed_update',{version:release.latestVersion});
                setMessage(zh?'已启动签名更新安装器，请按系统提示完成。':
                    'The verified installer was launched. Follow system prompts.');
            } else {
                await invoke('open_official_release_link',{
                    version:release.latestVersion,kind:'page',
                });
                onDismiss();
            }
        } catch(reason) {
            setMessage(null);
            setError((zh?'更新操作未完成：':'Update was not completed: ')+String(reason));
        } finally {setBusy(false);}
    };

    return <div role="presentation" className="fixed inset-0 z-[450] flex items-center justify-center p-4"
        style={{background:'rgba(6,12,24,.62)',backdropFilter:'blur(5px)'}}>
        <section role="dialog" aria-modal="true" aria-labelledby="startup-update-heading"
            className="ml-card w-full max-w-[450px] p-6 shadow-2xl border border-[var(--border-base)]">
            <div className="flex items-start justify-between gap-3">
                <div className="flex gap-3 items-center">
                    <div className="rounded-xl p-3 bg-[var(--bg-elevated)] text-[var(--primary)]">
                        <Download size={22}/>
                    </div>
                    <div>
                        <h2 id="startup-update-heading" className="text-lg font-bold">
                            {zh?'发现 MirroLeap 新版本':'New MirroLeap version'}
                        </h2>
                        <p className="text-sm font-semibold mt-1">
                            v{release.currentVersion} → v{release.latestVersion}
                            {release.prerelease && <span className="ml-muted text-xs ml-2">
                                {zh?'测试版':'Preview'}
                            </span>}
                        </p>
                    </div>
                </div>
                <button onClick={onDismiss} type="button"
                    aria-label={zh?'稍后更新':'Remind me next launch'}
                    className="ml-icon-button"><X size={18}/></button>
            </div>
            {release.changelog && <p className="ml-muted text-sm leading-relaxed mt-4 whitespace-pre-line line-clamp-4">
                {release.changelog}
            </p>}
            <p className="ml-muted text-xs mt-4 flex items-start gap-2">
                <ShieldCheck size={16} className="shrink-0 text-[var(--primary)]"/>
                {isNsisInstall
                    ? (running
                        ? (zh?'请先结束正在进行的投屏，之后即可一键安全升级。':
                            'Stop mirroring before installing the verified update.')
                        : (zh?'点击确认后才会下载。原生安装器将验证官方签名，不会静默升级。':
                            'Downloads only after your click. The native updater verifies the official signature.'))
                    : (zh?'当前安装方式不支持自动覆盖，将在浏览器打开对应官方下载。':
                        'Your installation uses safe manual downloads in the default browser.')}
            </p>
            {message && <p role="status" className="text-sm mt-3 text-emerald-600">
                {message}{progress!==null?' · '+progress+'%':''}
            </p>}
            {error && <p role="alert" className="text-sm mt-3 text-rose-500 break-words">{error}</p>}
            <div className="flex flex-wrap items-center gap-3 mt-6">
                <button type="button" onClick={()=>void proceed()} disabled={busy}
                    className="ml-primary-action flex-1 inline-flex items-center justify-center gap-2 rounded-xl py-3 font-bold">
                    {busy?<RefreshCw size={17} className="animate-spin"/>:
                        isNsisInstall?<Download size={17}/>:<ExternalLink size={17}/>}
                    {isNsisInstall?(zh?'立即安全更新':'Install verified update'):
                        zh?'查看新版下载':'Open official download'}
                </button>
                <button type="button" onClick={onDismiss}
                    className="ml-secondary-action rounded-xl px-4 py-3">
                    {zh?'稍后':'Later'}
                </button>
            </div>
        </section>
    </div>;
}
