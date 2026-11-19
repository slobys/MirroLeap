import {useEffect,useState,type MouseEvent} from 'react';
import {invoke} from '@tauri-apps/api/core';
import {Airplay,CheckCircle2,ChevronDown,ChevronUp,Download,Expand,ExternalLink,
    Info,LoaderCircle,MonitorPlay,ShieldCheck,Square} from 'lucide-react';

export type IosAirplayStatus = {
    supported:boolean;
    installed:boolean;
    running:boolean;
    installing:boolean;
    receiverName:string;
    engineVersion:string;
    compactMode:boolean;
    windowStyle?:string;
    qualityTier?:string;
    /** Actual received decoder texture, not the chosen advertisement cap. */
    sourceWidth?:number|null;
    sourceHeight?:number|null;
    /** Win32 physical SDL client area (may be smaller/larger than source). */
    displayWidth?:number|null;
    displayHeight?:number|null;
    /** Native mouse-through BGRA bezel is visibly aligned with the video. */
    frameAttached?:boolean;
    frameError?:string|null;
};
type Props={locale:string};
const windowStyles=[
    {value:'device-auto',zh:'真机边框 · 自动识别',en:'Device frame · auto'},
    {value:'device-ipad',zh:'iPad 真机边框',en:'iPad frame'},
    {value:'device-iphone',zh:'iPhone 真机边框',en:'iPhone frame'},
    {value:'clean',zh:'纯画面（无边框）',en:'Clean image only'},
    {value:'classic',zh:'接收器原版界面',en:'Original receiver UI'},
];
const qualityTiers=[
    {value:'adaptive',zh:'智能清晰 · 匹配窗口（推荐）',en:'Adaptive detail · match window (recommended)'},
    {value:'balanced',zh:'流畅 · 1920',en:'Balanced · 1920'},
    {value:'high',zh:'高清 · 2868',en:'High detail · 2868'},
    {value:'ultra',zh:'极致 · 3840',en:'Ultra · 3840'},
];
type Action='install'|'start'|'stop';

/** All iOS controls are embedded in the home page. No modal is created and
 * no install/start action is dispatched by rendering or by status polling. */
export default function IosReceiverPanel({locale}:Props) {
    const zh=locale.startsWith('zh');
    const copy=(cn:string,en:string)=>zh?cn:en;
    const [status,setStatus]=useState<IosAirplayStatus|null>(null);
    const [busy,setBusy]=useState<string|null>(null);
    const [error,setError]=useState<string|null>(null);
    const [helpOpen,setHelpOpen]=useState(false);
    const [feedback,setFeedback]=useState<string|null>(null);

    useEffect(()=>{
        let active=true;
        const refresh=async()=>{
            try{
                const next=await invoke<IosAirplayStatus>('ios_airplay_status');
                if(active) setStatus(next);
            }catch(e){if(active) setError(String(e));}
        };
        void refresh();
        // Process liveness only; do not claim an iPhone is connected.
        const timer=setInterval(()=>void refresh(),4000);
        return ()=>{active=false;clearInterval(timer);};
    },[]);

    const run=async(action:Action)=>{
        if(busy) return;
        setBusy(action);setError(null);setFeedback(null);
        try {
            setStatus(await invoke<IosAirplayStatus>('ios_airplay_'+action));
        } catch(e){
            setError(String(e));
            try{setStatus(await invoke<IosAirplayStatus>('ios_airplay_status'));}
            catch{/* Preserve original error. */}
        } finally {setBusy(null);}
    };
    const setStyle=async(style:string)=>{
        if(busy || !windowStyles.some(option=>option.value===style)) return;
        setBusy('style');setError(null);setFeedback(null);
        try{
            setStatus(await invoke<IosAirplayStatus>('ios_airplay_set_window_style',{style}));
        }catch(e){setError(String(e));}
        finally{setBusy(null);}
    };
    const setQuality=async(tier:string)=>{
        if(busy || status?.running || !qualityTiers.some(option=>option.value===tier)) return;
        setBusy('quality');setError(null);setFeedback(null);
        try{
            setStatus(await invoke<IosAirplayStatus>('ios_airplay_set_quality_tier',{tier}));
        }catch(e){setError(String(e));}
        finally{setBusy(null);}
    };
    const fullscreen=async()=>{
        if(busy || !status?.running)return;
        setBusy('fullscreen');setError(null);
        try{
            await invoke<IosAirplayStatus>('ios_airplay_window_action',{action:'fullscreen'});
            setFeedback(copy('已发送全屏切换请求，可在接收窗口按 F11。',
                'Fullscreen request sent; F11 works in the receiver window.'));
        }catch(e){setError(String(e));}
        finally{setBusy(null);}
    };
    const openSource=async(event:MouseEvent<HTMLAnchorElement>)=>{
        if(!('__TAURI_INTERNALS__' in window))return;
        event.preventDefault();
        try{await invoke('ios_airplay_open_source');}
        catch(e){setError(String(e));}
    };
    const working=busy!==null || Boolean(status?.installing);
    const isRunning=Boolean(status?.running);
    const style=status?.windowStyle || (status?.compactMode===false?'classic':'device-auto');
    const quality=status?.qualityTier || 'adaptive';
    const canChangeStyle=!working && Boolean(status?.installed) && (!isRunning || style!=='classic');

    return <section id="ios-airplay-home" aria-label={copy('iPhone / iPad 无线投屏','iPhone / iPad wireless mirroring')}
        className="ml-card p-3 sm:p-4 mb-3" data-testid="ios-home-panel">
        <div className="flex flex-wrap items-center gap-3">
            <span className="p-2.5 rounded-xl bg-emerald-500/10 text-emerald-500"><Airplay size={22}/></span>
            <div className="min-w-0 flex-1">
                <h3 className="text-sm font-bold">{copy('iPhone / iPad 无线投屏','iPhone / iPad wireless mirroring')}</h3>
                <div className="ml-muted text-xs mt-1 flex items-center gap-1.5" role="status">
                    {isRunning?<CheckCircle2 size={13} className="text-emerald-500"/>:
                        <span className="w-2 h-2 rounded-full bg-zinc-400" aria-hidden="true"/>}
                    {!status?copy('检测接收组件中…','Checking receiver…'):
                        !status.supported?copy('仅支持 Windows x64','Windows x64 only'):
                        !status.installed?copy('首次使用需要安装接收组件','Install receiver for first use'):
                        isRunning && status.sourceWidth && status.sourceHeight
                            ? copy('已接收镜像视频帧','Receiving mirroring frames'):
                        isRunning?copy('接收器已启动 · 等待手机连接','Receiver running · awaiting device'):
                        copy('已就绪 · 点击启动','Ready · click Start')}
                </div>
            </div>
            {!status?.installed && status?.supported && <button type="button"
                disabled={working} onClick={()=>void run('install')}
                className="ml-primary-action !w-auto px-3 !min-h-10 inline-flex items-center gap-2 disabled:opacity-50">
                {working?<LoaderCircle size={16} className="animate-spin"/>:<Download size={16}/>}
                {copy('安装 iOS 接收器','Install iOS receiver')}</button>}
            {status?.installed && status.supported && <button type="button"
                disabled={working} onClick={()=>void run(isRunning?'stop':'start')}
                className={(isRunning?'ml-stop-action':'ml-primary-action')+
                    ' !w-auto px-4 !min-h-10 inline-flex items-center justify-center gap-2 disabled:opacity-50'}>
                {working?<LoaderCircle size={16} className="animate-spin"/>:
                    isRunning?<Square size={15}/>:<MonitorPlay size={17}/>}
                {isRunning?copy('停止接收','Stop receiver'):copy('启动 iOS 投屏','Start iOS mirroring')}
            </button>}
        </div>

        {status?.supported && status.installed && <div className="mt-3 flex flex-wrap items-end gap-3">
            <label className="text-xs font-semibold min-w-[160px] flex-1">
                {copy('投屏窗口风格','Window style')}
                <select aria-label={copy('iOS 投屏窗口风格','iOS window style')}
                    value={style} disabled={!canChangeStyle}
                    onChange={event=>void setStyle(event.target.value)}
                    className="ml-select mt-1 w-full">
                    {windowStyles.map(option=><option key={option.value} value={option.value}
                        disabled={isRunning && option.value==='classic'}>
                        {zh?option.zh:option.en}</option>)}
                </select>
            </label>
            <label className="text-xs font-semibold min-w-[145px] flex-1">
                {copy('iOS 画质','iOS image quality')}
                <select aria-label={copy('iOS 画质','iOS image quality')}
                    value={quality} disabled={working || isRunning}
                    onChange={event=>void setQuality(event.target.value)}
                    className="ml-select mt-1 w-full">
                    {qualityTiers.map(option=><option key={option.value} value={option.value}>
                        {zh?option.zh:option.en}</option>)}
                </select>
            </label>
            {isRunning && <button type="button" onClick={()=>void fullscreen()} disabled={working}
                className="ml-secondary-action !min-h-10 px-3 inline-flex items-center gap-2 text-xs">
                <Expand size={15}/>{copy('全屏','Fullscreen')}</button>}
        </div>}
        {status?.installed && <p className="ml-muted mt-2 text-[11px] leading-5" role="note">
            {copy('真机边框使用 Windows 原生透明图层，避免跨进程绑定造成主窗口 DPI 异常。画面保持原样，外壳不参与视频解码。可运行中切换真机/纯画面。',
                'The bezel uses a native Windows transparent layer, not cross-process WebView parenting. Video is untouched. Switch styles live.')}
        </p>}
        {status?.installed && <p className="ml-muted text-[11px] leading-5" role="note">
            {copy('投屏窗口右上方新增「—」最小化与「×」关闭按钮。智能清晰度会参考上次实际窗口尺寸，在下一次启动时匹配 iOS 视频分辨率，减少过度缩小造成的图标锯齿。',
                'Native minimize and close buttons appear above the mirror’s top right. Adaptive detail uses the last measured window pixels on the next launch to reduce excessive downscaling.')}
        </p>}
        {isRunning && style.startsWith('device-') && <p role="status"
            data-testid="ios-frame-attachment" className="ml-muted text-[11px] mt-1">
            {status?.frameAttached
                ? copy('真机边框：原生图层已对齐','Device frame: native layer aligned')
                : copy('真机边框：原生图层未显示，正在同步视频位置',
                    'Device frame: native layer not visible; synchronizing video position')}
        </p>}
        {isRunning && !status?.frameAttached && status?.frameError &&
            <p role="alert" className="text-red-500 text-[11px] leading-5 mt-1"
                data-testid="ios-frame-error">{status.frameError}</p>}
        {isRunning && <p className="ml-muted text-[11px] leading-5" role="note" data-testid="ios-actual-resolution">
            {status?.sourceWidth && status?.sourceHeight
                ? copy('实际解码画面：','Decoded stream: ')
                    + status.sourceWidth + ' × ' + status.sourceHeight
                    + (status.displayWidth && status.displayHeight
                        ? ' · '+copy('显示窗口：','Window: ')
                            +status.displayWidth+' × '+status.displayHeight : '')
                : copy('等待第一帧；画质档位只是接收上限，并非实际画面分辨率。',
                    'Waiting for the first frame. Quality settings are ceilings, not actual frame size.')}
        </p>}
        {isRunning && status?.sourceWidth && status.sourceHeight &&
            status.displayWidth && status.displayHeight &&
            (status.displayWidth>status.sourceWidth*1.05 ||
                status.displayHeight>status.sourceHeight*1.05) &&
            <p role="note" className="text-amber-500 text-[11px] leading-5">
                {copy('当前窗口在放大源画面，可能出现模糊。可以缩小接收窗口，或停止后尝试更高画质档位。',
                    'The window is upscaling received pixels. Shrink it or try a higher quality tier after stopping.')}
            </p>}
        {isRunning && status?.sourceWidth && status.sourceHeight &&
            status.displayWidth && status.displayHeight &&
            Math.min(status.sourceWidth/status.displayWidth,
                status.sourceHeight/status.displayHeight)>1.7 &&
            <p role="note" data-testid="ios-downscale-warning"
                className="text-amber-500 text-[11px] leading-5">
                {copy('当前高分辨率画面正在缩小超过 1.7 倍，普通线性采样可能让 App 图标边缘出现锯齿。建议停止接收后对比「高清 2868」与「流畅 1920」，选择在当前窗口看起来更自然的档位；不一定是 4K 最清晰。',
                    'The decoded frame is reduced over 1.7×; linear sampling can alias sharp app icons. Stop and compare 2868 versus 1920; 4K is not always sharpest at this window size.')}
            </p>}
        {error && <p role="alert" className="mt-2 text-xs text-red-500 break-words">{error}</p>}
        {isRunning && <p className="ml-muted mt-1 text-[11px]" role="note">
            {copy('点击主页「停止接收」即可关闭投屏；关闭 MirroLeap 主窗口也会停止接收器。旧版本残留的独立接收窗口可点击后按 Alt+F4。',
                'Use Stop receiver to end mirroring; closing MirroLeap also stops its receiver. For an old orphaned receiver window, focus it and press Alt+F4.')}
        </p>}
        {feedback && <p role="status" className="mt-2 text-xs">{feedback}</p>}
        <div className="mt-2 flex items-center justify-between flex-wrap gap-2">
            <button type="button" aria-expanded={helpOpen} onClick={()=>setHelpOpen(o=>!o)}
                className="ml-muted hover:underline flex items-center gap-1 text-xs">
                <Info size={14}/>{copy('连接说明','How to connect')}
                {helpOpen?<ChevronUp size={13}/>:<ChevronDown size={13}/>}
            </button>
            <a href="https://github.com/moieric11/AirPlay-Windows" target="_blank"
                rel="noopener noreferrer" onClick={event=>void openSource(event)}
                className="ml-muted hover:underline inline-flex gap-1 items-center text-[11px]">
                <ShieldCheck size={13}/>{copy('GPL-3.0 开源接收器','GPL-3.0 receiver')}
                <ExternalLink size={12}/></a>
        </div>
        {helpOpen && <div className="mt-3 rounded-xl border border-zinc-500/20 p-3 text-xs leading-6">
            <p>{copy('手机与电脑连接同一 Wi-Fi（非访客隔离网络）。首次启动若 Windows 防火墙提示，只允许受信任的“专用网络”。',
                'Keep phone and PC on the same Wi-Fi, without guest isolation. Allow trusted Private networks if Windows Firewall prompts.')}</p>
            <p>{copy('在 iPhone / iPad 控制中心 → 屏幕镜像 → 选择 AirPlay-Windows。停止时在手机选择停止镜像。',
                'On iPhone/iPad: Control Center → Screen Mirroring → AirPlay-Windows. Stop from the phone.')}</p>
            <p className="ml-muted">{copy('接收器运行不等于 iPhone 已连接；不支持 DRM 受保护视频和 iOS 触控控制。',
                'Receiver running does not mean connected. DRM-protected video and iOS touch control are unsupported.')}</p>
        </div>}
    </section>;
}
