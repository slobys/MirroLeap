import {useEffect,useState,type FormEvent,type MouseEvent} from 'react';
import {invoke} from '@tauri-apps/api/core';
import {Airplay,CheckCircle2,ChevronDown,ChevronUp,Download,Expand,ExternalLink,
    Info,LoaderCircle,MonitorPlay,Pencil,ShieldCheck,Square,Usb} from 'lucide-react';

export type IosAirplayStatus = {
    supported:boolean;
    installed:boolean;
    running:boolean;
    installing:boolean;
    receiverName:string;
    nextReceiverName?:string|null;
    receiverIp?:string|null;
    receiverReady?:boolean;
    engineVersion:string;
    compactMode:boolean;
    windowStyle?:string;
    qualityTier?:string;
    windowSize?:string;
    qualityPending?:boolean;
    usbStatus?:string;
    usbSourceWidth?:number|null;
    usbSourceHeight?:number|null;
    diagnostics?:{
        usbVideo:string;
        bluetooth:string;
        bluetoothTransport?:'usb'|'wifi'|null;
    };
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
const windowSizes=[
    {value:'auto',zh:'随画质（推荐）',en:'Follow quality (recommended)'},
    {value:'small',zh:'小窗口',en:'Small'},
    {value:'medium',zh:'中窗口',en:'Medium'},
    {value:'large',zh:'大窗口',en:'Large'},
    {value:'source',zh:'按源像素（超出时缩小）',en:'Source pixels (fit if needed)'},
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
    const [detailsOpen,setDetailsOpen]=useState(false);
    const [exporting,setExporting]=useState(false);
    const [exportResult,setExportResult]=useState<{ok:boolean;message:string}|null>(null);
    const [feedback,setFeedback]=useState<string|null>(null);
    const [receiverNameDraft,setReceiverNameDraft]=useState<string|null>(null);
    const [nameError,setNameError]=useState<string|null>(null);

    useEffect(()=>{
        let active=true;
        const refresh=async()=>{
            try{
                const next=await invoke<IosAirplayStatus>('ios_airplay_status');
                if(active) setStatus(next);
            }catch(e){if(active) setError(String(e));}
        };
        void refresh();
        const timer=setInterval(()=>void refresh(),1500);
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
        if(busy || !qualityTiers.some(option=>option.value===tier)) return;
        setBusy('quality');setError(null);setFeedback(null);
        try{
            setStatus(await invoke<IosAirplayStatus>('ios_airplay_set_quality_tier',{tier}));
        }catch(e){setError(String(e));}
        finally{setBusy(null);}
    };
    const setSize=async(size:string)=>{
        if(busy || !windowSizes.some(option=>option.value===size))return;
        setBusy('size');setError(null);setFeedback(null);
        try{setStatus(await invoke<IosAirplayStatus>('ios_airplay_set_window_size',{size}));}
        catch(e){setError(String(e));}
        finally{setBusy(null);}
    };
    const saveReceiverName=async(event:FormEvent<HTMLFormElement>)=>{
        event.preventDefault();
        if(busy || receiverNameDraft===null)return;
        const name=receiverNameDraft.trim();
        if(!name || new TextEncoder().encode(name).length>50 || /[\x00-\x1f\x7f-\x9f.\\]/.test(name)){
            setNameError(copy('请输入名称。最多 50 个 UTF-8 字节，不能包含英文句点、反斜杠或控制字符。',
                'Enter a name up to 50 UTF-8 bytes, without periods, backslashes, or control characters.'));
            return;
        }
        setBusy('receiver-name');setError(null);setNameError(null);setFeedback(null);
        try{
            setStatus(await invoke<IosAirplayStatus>('ios_airplay_set_receiver_name',{name}));
            setReceiverNameDraft(null);
        }catch(e){setNameError(String(e));}
        finally{setBusy(null);}
    };
    const installUsbDriver=async()=>{
        if(busy)return;
        setBusy('usb');setError(null);setFeedback(null);
        try{
            await invoke('ios_usb_install_driver');
            setFeedback(copy('驱动已安装。请重新插拔数据线。','Driver installed. Unplug and reconnect the cable.'));
        }catch(e){
            setError(String(e));
        }finally{setBusy(null);}
    };
    const controlUsb=async(start:boolean)=>{
        if(busy)return;
        setBusy('usb-session');setError(null);setFeedback(null);
        try{
            await invoke('ios_usb_control',{start});
            setStatus(await invoke<IosAirplayStatus>('ios_airplay_status'));
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
    const exportDiagnostics=async()=>{
        if(exporting)return;
        setExporting(true);
        setExportResult(null);
        try{
            const path=await invoke<string>('ios_airplay_export_diagnostics');
            const file=path.split(/[\\/]/).pop();
            setExportResult({ok:true,message:copy(`已保存到「下载」文件夹 · ${file}`,
                `Saved to Downloads · ${file}`)});
        }catch{
            setExportResult({ok:false,message:copy('诊断日志未能保存。请确认「下载」文件夹可写后重试。',
                'Could not save diagnostics. Check that Downloads is writable and try again.')});
        }finally{setExporting(false);}
    };
    const working=busy!==null || Boolean(status?.installing);
    const hasUsb=Boolean(status?.supported && status.usbStatus && status.usbStatus!=='not_bundled');
    const usbState=status?.usbStatus;
    const usbActive=usbState==='streaming' || usbState==='connecting' || usbState==='reconnecting';
    const usbCanStart=usbState==='ready' || usbState==='interrupted';
    const usbConnected=Boolean(usbState && [
        'ready','detecting','authorization_pending','unlock','capture_driver','busy','connecting','streaming','stopping',
    ].includes(usbState));
    const usbSummary=usbState==='streaming'?copy('USB连接','USB mirroring'):
        usbState==='reconnecting'?copy('USB重连中','Reconnecting over USB'):
        usbState==='interrupted'?copy('USB投屏已中断','USB mirroring interrupted'):
        usbConnected?copy('USB已连接','USB connected'):
        usbState==='waiting'?copy('USB未连接','USB disconnected'):
        usbState==='starting'?copy('正在检测 USB…','Checking USB…'):
        usbState==='apple_driver'?copy('USB 需要 Apple Devices','USB requires Apple Devices'):
        copy('USB 暂不可用','USB unavailable');
    const usbAttention=usbState==='detecting'?copy('识别设备中','Detecting device'):
        usbState==='authorization_pending'?copy('待确认授权','Checking authorization'):
        usbState==='unlock'?copy('请解锁设备','Unlock device'):
        usbState==='capture_driver'?copy('需安装驱动','Driver required'):
        usbState==='busy'?copy('设备占用中','Device busy'):null;
    const isRunning=Boolean(status?.running);
    const hasVideo=Boolean(isRunning && status?.sourceWidth && status.sourceHeight);
    const usbVideoPhase=status?.diagnostics?.usbVideo ?? (usbState==='streaming'?'receiving':'idle');
    const usbPicture=({
        waiting:copy('USB · 等待画面','USB · waiting for video'),
        receiving:copy('USB · 接收画面中','USB · receiving video'),
        paused:copy('USB · 暂无新画面，等待恢复','USB · no new frames, waiting to resume'),
        recovering:copy('USB · 正在恢复画面','USB · recovering video'),
        interrupted:copy('USB · 画面已中断','USB · video interrupted'),
    } as Record<string,string>)[usbVideoPhase];
    const bluetoothPhase=status?.diagnostics?.bluetooth ?? 'off';
    const bluetoothStatus=({
        off:copy('未连接控制','Controls not connected'),
        ready:copy('已连接，键鼠可用','Connected · keyboard and mouse ready'),
        keyboard_only:copy('已连接，等待鼠标','Connected · waiting for the mouse'),
        reconnecting:copy('等待设备重新连接','Waiting for the device to reconnect'),
        waiting:copy('等待设备配对','Waiting for pairing'),
        confirm:copy('等待确认设备','Waiting for device confirmation'),
        multiple:copy('请只连接一台 iOS 设备','Connect only one iOS device'),
    } as Record<string,string>)[bluetoothPhase];
    const style=status?.windowStyle || (status?.compactMode===false?'classic':'device-auto');
    const quality=status?.qualityTier || 'adaptive';
    const canChangeStyle=!working && Boolean(status?.installed) && (!isRunning || style!=='classic');

    return <section id="ios-airplay-home" aria-label={copy('iPhone / iPad 投屏','iPhone / iPad mirroring')}
        className="ml-card p-3 sm:p-4 mb-3" data-testid="ios-home-panel">
        <div className="flex flex-wrap items-center gap-3">
            <span className="p-2.5 rounded-xl bg-emerald-500/10 text-emerald-500"><Airplay size={22}/></span>
            <div className="min-w-0 flex-1 flex flex-wrap items-center gap-x-6 gap-y-1 sm:gap-x-12">
                <div className="min-w-0">
                    <h3 className="text-sm font-bold">{copy('iPhone / iPad 投屏','iPhone / iPad mirroring')}</h3>
                    <div className="ml-muted text-xs mt-1 flex items-center gap-1.5" role="status">
                        {isRunning?<CheckCircle2 size={13} className="text-emerald-500"/>:
                            <span className="w-2 h-2 rounded-full bg-zinc-400" aria-hidden="true"/>}
                        {!status?copy('检测接收组件中…','Checking receiver…'):
                            !status.supported?copy('仅支持 Windows x64','Windows x64 only'):
                            !status.installed?copy('首次使用需要安装接收组件','Install receiver for first use'):
                            hasVideo
                                ? copy('已接收镜像视频帧','Receiving mirroring frames'):
                            isRunning && status.receiverReady===false
                                ? copy('正在启动 AirPlay 接收器','Starting AirPlay receiver'):
                            isRunning?copy('接收器已启动 · 等待手机连接','Receiver running · awaiting device'):
                            copy('已就绪 · 点击启动','Ready · click Start')}
                    </div>
                </div>
                {(hasUsb || hasVideo) && <div className="flex min-w-0 flex-wrap items-center gap-x-4 gap-y-1">
                    {hasUsb && <p className="flex items-center gap-1.5 text-xs" role="status">
                        {usbConnected?<CheckCircle2 size={14} className="shrink-0 text-emerald-500" aria-hidden="true"/>:
                            <Usb size={14} className="ml-muted shrink-0" aria-hidden="true"/>}
                        <span className={usbConnected?'font-semibold':'ml-muted'}>{usbSummary}</span>
                        {usbAttention && <span className="ml-muted"> · {usbAttention}</span>}
                    </p>}
                    {hasVideo && <p className="flex items-center gap-1.5 text-xs font-semibold" role="status">
                        <CheckCircle2 size={14} className="shrink-0 text-emerald-500" aria-hidden="true"/>
                        <span>{copy('WiFi连接','Wi-Fi mirroring')}</span>
                    </p>}
                </div>}
            </div>
            {hasUsb && <button type="button" onClick={()=>void controlUsb(!usbActive)}
                disabled={working || (!usbActive && !usbCanStart)}
                className={(usbActive?'ml-stop-action':'ml-primary-action')+
                    ' ml-ios-usb-action !w-auto px-4 !min-h-10 inline-flex items-center justify-center gap-2'}>
                {usbActive?<Square size={15}/>:<Usb size={17}/>}
                {usbActive?copy('停止 USB 投屏','Stop USB mirroring'):copy('USB投屏','USB mirroring')}
            </button>}
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

        {status?.installed && (!hasVideo || receiverNameDraft!==null || status.nextReceiverName) && <div data-testid="ios-connection-info"
            className="mt-3 border-t border-current/10 pt-3 text-xs">
            <dl className="flex flex-wrap gap-x-7 gap-y-2">
                <div className="min-w-0">
                    <dt className="ml-muted">{copy('接收器名称','Receiver name')}</dt>
                    <dd className="mt-1">
                        {receiverNameDraft===null?<div className="flex flex-wrap items-center gap-x-2">
                            <span className="min-w-0 select-text break-all text-sm font-semibold">{status.receiverName}</span>
                            <button type="button" disabled={working}
                                aria-label={copy('编辑接收器名称','Edit receiver name')}
                                title={copy('编辑接收器名称','Edit receiver name')}
                                onClick={()=>{setReceiverNameDraft(status.nextReceiverName||status.receiverName);setNameError(null);}}
                                className="ml-muted inline-flex min-h-8 min-w-8 items-center justify-center rounded-md hover:bg-current/10">
                                <Pencil size={14} aria-hidden="true"/>
                            </button>
                        </div>:<form onSubmit={event=>void saveReceiverName(event)}
                            className="space-y-1">
                            <div className="flex flex-wrap items-center gap-1.5">
                                <input aria-label={copy('接收器名称','Receiver name')} autoFocus
                                    type="text" autoComplete="off" disabled={working} aria-invalid={Boolean(nameError)}
                                    aria-describedby={nameError?'ios-receiver-name-error':undefined}
                                    value={receiverNameDraft} onChange={event=>setReceiverNameDraft(event.target.value)}
                                    className="ml-select min-w-0 max-w-full w-44 !min-h-8 px-2 text-xs"/>
                                <button type="submit" disabled={working || !receiverNameDraft.trim()}
                                    className="ml-primary-action !w-auto !min-h-8 px-2.5 text-xs">
                                    {busy==='receiver-name'?copy('保存中…','Saving…'):copy('保存','Save')}
                                </button>
                                <button type="button" disabled={working}
                                    className="ml-muted min-h-8 px-2 hover:underline"
                                    onClick={()=>{setReceiverNameDraft(null);setNameError(null);}}>
                                    {copy('取消','Cancel')}
                                </button>
                            </div>
                            {nameError && <p id="ios-receiver-name-error" role="alert"
                                className="text-red-500 break-words">{nameError}</p>}
                            {isRunning && !hasVideo && <p className="ml-muted">{copy(
                                '新名称在接收器重启后生效。','The new name takes effect when the receiver restarts.')}</p>}
                        </form>}
                        {status.nextReceiverName && <p className="ml-muted mt-1" role="status">{copy(
                            `下次启动将使用「${status.nextReceiverName}」。`,
                            `Next start will use “${status.nextReceiverName}”.`)}</p>}
                    </dd>
                </div>
                {isRunning && <div className="min-w-0">
                    <dt className="ml-muted">{copy('局域网 IP','Local IP')}</dt>
                    <dd className="mt-1 select-text break-all text-sm font-semibold tabular-nums">
                        {status.receiverIp || (status.receiverReady===false
                            ? copy('正在获取…','Detecting…') : copy('未检测到','Unavailable'))}
                    </dd>
                </div>}
            </dl>
            {isRunning && !hasVideo && <p className="ml-muted mt-2 leading-5">{copy(
                '让 iPhone 或 iPad 和电脑连接同一局域网，然后在设备的「控制中心 → 屏幕镜像」选择上方接收器。',
                'Connect your iPhone or iPad to the same local network as this PC. In Control Center → Screen Mirroring, choose the receiver above.')}</p>}
        </div>}

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
                    value={quality} disabled={working}
                    onChange={event=>void setQuality(event.target.value)}
                    className="ml-select mt-1 w-full">
                    {qualityTiers.map(option=><option key={option.value} value={option.value}>
                        {zh?option.zh:option.en}</option>)}
                </select>
            </label>
            <label className="text-xs font-semibold min-w-[145px] flex-1">
                {copy('投屏窗口大小','Mirror window size')}
                <select aria-label={copy('iOS 投屏窗口大小','iOS mirror window size')}
                    value={status.windowSize||'auto'} disabled={working || style==='classic'}
                    onChange={event=>void setSize(event.target.value)} className="ml-select mt-1 w-full">
                    {windowSizes.map(option=><option key={option.value} value={option.value}>
                        {zh?option.zh:option.en}</option>)}
                </select>
            </label>
            {hasVideo && <button type="button" onClick={()=>void fullscreen()} disabled={working}
                className="ml-secondary-action !min-h-10 px-3 inline-flex items-center gap-2 text-xs">
                <Expand size={15}/>{copy('全屏','Fullscreen')}</button>}
        </div>}
        {status?.qualityPending && isRunning && <p className="ml-muted mt-2 text-xs" role="status">
            {copy('新画质已保存。重新启动接收器后，iPhone 或 iPad 会按新的上限重新协商画面。',
                'Quality saved. Restart the receiver to negotiate the new limit with your iPhone or iPad.')}
        </p>}
        <div className="mt-3 flex justify-end border-t border-current/10 pt-2">
            <button type="button" aria-expanded={detailsOpen} aria-controls="ios-receiver-details"
                onClick={()=>setDetailsOpen(open=>!open)}
                className="ml-muted min-h-8 inline-flex items-center gap-1.5 px-1 text-xs hover:underline">
                <Info size={14} aria-hidden="true"/>{copy('投屏详情','Mirroring details')}
                {detailsOpen?<ChevronUp size={14} aria-hidden="true"/>:<ChevronDown size={14} aria-hidden="true"/>}
            </button>
        </div>
        {error && <p role="alert" className="mt-2 text-xs text-red-500 break-words">{error}</p>}
        {feedback && <p role="status" className="mt-2 text-xs">{feedback}</p>}
        <div id="ios-receiver-details" hidden={!detailsOpen} className="mt-2 space-y-2 text-xs leading-5">
            <dl className="grid gap-x-6 gap-y-3 pb-2 sm:grid-cols-3">
                <div className="min-w-0">
                    <dt className="font-semibold">{copy('USB 状态','USB status')}</dt>
                    <dd className="ml-muted mt-1" role="status" data-testid="ios-usb-detail">{hasUsb && usbState?({
                        starting:copy('正在检测 USB 设备…','Checking for USB devices…'),
                        waiting:copy('请插入 USB 数据线。','Connect a USB data cable.'),
                        ready:copy('已连接，点击上方「USB投屏」开始。','Connected. Click USB mirroring above to start.'),
                        detecting:copy('Windows 已检测到 USB。正在等待 Apple 设备就绪。','Windows detected the USB connection. Waiting for the Apple device.'),
                        authorization_pending:copy('请解锁设备。如果设备弹出信任提示，再点「信任」。','Unlock the device. Tap Trust if the device prompts you.'),
                        unlock:copy('设备已有配对记录。请解锁并保持连接，等待 USB 投屏就绪。','A pairing record exists. Unlock the device and keep it connected while USB mirroring gets ready.'),
                        apple_driver:copy('首次使用需安装 Apple Devices。','Install Apple Devices for first-time setup.'),
                        capture_driver:copy('此设备需要一次性安装 USB 捕获驱动。','This device needs a one-time USB capture driver setup.'),
                        busy:copy('USB 捕获设备正在使用，请先关闭其他投屏程序。','USB capture is busy. Close any other mirroring app.'),
                        connecting:copy('正在打开 USB 捕获连接…','Opening the USB capture connection…'),
                        reconnecting:copy('正在恢复与同一设备的 USB 捕获连接。','Restoring USB capture for the same device.'),
                        interrupted:copy('捕获连接已结束，可以点击「USB投屏」重试。','Capture ended. Click USB mirroring to try again.'),
                        stopping:copy('正在关闭 USB 捕获连接…','Closing the USB capture connection…'),
                        streaming:copy('USB 已连接','USB connected'),
                        unavailable:copy('暂时无法接收 USB 画面，请解锁设备并重新插线。','USB video is unavailable. Unlock the device and reconnect the cable.'),
                    } as Record<string,string>)[usbState]:copy('USB 组件不可用','USB component unavailable')}</dd>
                    {usbState==='apple_driver' && <dd className="mt-2"><a href="https://apps.microsoft.com/detail/9np83lwlpz9k"
                        target="_blank" rel="noreferrer" className="ml-secondary-action !min-h-10 px-3 text-xs inline-flex items-center gap-2">
                        Apple Devices <ExternalLink size={14} aria-hidden="true"/></a></dd>}
                    {usbState==='capture_driver' && <dd className="mt-2"><button type="button" disabled={working} onClick={()=>void installUsbDriver()}
                        className="ml-secondary-action !min-h-10 px-3 text-xs">
                        {busy==='usb'?copy('正在安装…','Installing…'):copy('安装 USB 驱动','Install USB driver')}
                    </button></dd>}
                </div>
                <div className="min-w-0">
                    <dt className="font-semibold">{copy('画面状态','Video status')}</dt>
                    <dd className="ml-muted mt-1" role="status" data-testid="ios-video-detail">
                        {usbPicture && <p>{usbPicture}</p>}
                        {status?.usbSourceWidth && status.usbSourceHeight &&
                            <p>{copy('USB 视频源 ','USB video source ')}{status.usbSourceWidth} × {status.usbSourceHeight}</p>}
                        {isRunning && <p>{hasVideo?copy('Wi-Fi · 已接收画面','Wi-Fi · video received'):
                            copy('Wi-Fi · 等待画面','Wi-Fi · waiting for video')}</p>}
                        {!usbPicture && !isRunning && copy('未接收投屏画面','No mirroring video')}
                    </dd>
                </div>
                <div className="min-w-0">
                    <dt className="font-semibold">{copy('蓝牙控制','Bluetooth controls')}</dt>
                    <dd className="ml-muted mt-1" role="status" data-testid="ios-bluetooth-detail">
                        {status?.diagnostics?.bluetoothTransport==='usb'?'USB · ':
                            status?.diagnostics?.bluetoothTransport==='wifi'?'Wi-Fi · ':null}
                        {bluetoothStatus}
                    </dd>
                </div>
            </dl>
            {status?.supported && <div className="flex flex-wrap items-center gap-x-3 gap-y-2 border-t border-current/10 pt-2">
                <button type="button" disabled={exporting} onClick={()=>void exportDiagnostics()}
                    className="ml-secondary-action !min-h-9 px-3 text-xs inline-flex items-center gap-1.5">
                    <Download size={14} aria-hidden="true"/>
                    {exporting?copy('正在导出…','Exporting…'):copy('导出诊断日志','Export diagnostics')}
                </button>
                {exportResult && <p className={(exportResult.ok?'ml-muted':'text-red-500')+' min-w-0 break-words'}
                    role={exportResult.ok?'status':'alert'}>{exportResult.message}</p>}
            </div>}
            {status?.installed && <p className="ml-muted mt-2 text-[11px] leading-5" role="note">
                {copy('真机边框使用 Windows 原生透明图层，避免跨进程绑定造成主窗口 DPI 异常。画面保持原样，外壳不参与视频解码。可运行中切换真机/纯画面。',
                    'The bezel uses a native Windows transparent layer, not cross-process WebView parenting. Video is untouched. Switch styles live.')}
            </p>}
            {status?.installed && <p className="ml-muted text-[11px] leading-5" role="note">
                {copy('连接后按设备比例打开投屏窗口。智能清晰会在接收器重启后匹配上次的实际窗口像素。',
                    'The mirror window opens at the device’s aspect ratio after connecting. Adaptive detail matches the last window size after the receiver restarts.')}
            </p>}
            {isRunning && (hasVideo || status?.frameError) && style.startsWith('device-') && <p role="status"
                data-testid="ios-frame-attachment" className="ml-muted text-[11px] mt-1">
                {status?.frameAttached
                    ? copy('真机边框：原生图层已对齐','Device frame: native layer aligned')
                    : copy('真机边框：原生图层未显示，正在同步视频位置',
                        'Device frame: native layer not visible; synchronizing video position')}
            </p>}
            {isRunning && !status?.frameAttached && status?.frameError &&
                <p role="alert" className="text-red-500 text-[11px] leading-5 mt-1"
                    data-testid="ios-frame-error">{status.frameError}</p>}
            {hasVideo && status && <p className="ml-muted text-[11px] leading-5" role="note" data-testid="ios-actual-resolution">
                {copy('实际解码画面：','Decoded stream: ')
                        + status.sourceWidth + ' × ' + status.sourceHeight
                        + (status.displayWidth && status.displayHeight
                            ? ' · '+copy('显示窗口：','Window: ')
                                +status.displayWidth+' × '+status.displayHeight : '')}
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
            {isRunning && <p className="ml-muted mt-1 text-[11px]" role="note">
                {copy('点击主页「停止接收」即可关闭投屏；关闭 MirroLeap 主窗口也会停止接收器。旧版本残留的独立接收窗口可点击后按 Alt+F4。',
                    'Use Stop receiver to end mirroring; closing MirroLeap also stops its receiver. For an old orphaned receiver window, focus it and press Alt+F4.')}
            </p>}
            <div className="rounded-xl border border-current/10 p-3 space-y-1">
                <p>{copy('手机与电脑连接同一 Wi-Fi（非访客隔离网络）。首次启动若 Windows 防火墙提示，只允许受信任的“专用网络”。',
                    'Keep phone and PC on the same Wi-Fi, without guest isolation. Allow trusted Private networks if Windows Firewall prompts.')}</p>
                <p>{copy(`在 iPhone / iPad 控制中心 → 屏幕镜像 → 选择 ${status?.receiverName || 'AirPlay-Windows'}。停止时在手机选择停止镜像。`,
                    `On iPhone/iPad: Control Center → Screen Mirroring → ${status?.receiverName || 'AirPlay-Windows'}. Stop from the phone.`)}</p>
                <p className="ml-muted">{copy('接收器运行不等于 iPhone 已连接；不支持 DRM 受保护视频和 iOS 触控控制。',
                    'Receiver running does not mean connected. DRM-protected video and iOS touch control are unsupported.')}</p>
            </div>
            <a href="https://github.com/moieric11/AirPlay-Windows" target="_blank"
                rel="noopener noreferrer" onClick={event=>void openSource(event)}
                className="ml-muted hover:underline inline-flex gap-1 items-center text-[11px]">
                <ShieldCheck size={13}/>{copy('GPL-3.0 开源接收器','GPL-3.0 receiver')}
                <ExternalLink size={12}/></a>
        </div>
    </section>;
}
