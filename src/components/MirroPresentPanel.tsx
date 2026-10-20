import {useEffect, useRef, useState} from 'react';
import {invoke} from '@tauri-apps/api/core';
import {listen} from '@tauri-apps/api/event';
import {Eye, Focus, Maximize, Move, RefreshCw, ScanSearch, XCircle} from 'lucide-react';
import {useI18n} from '../i18n';

type PresentMode = 'off' | 'spotlight' | 'magnifier' | 'live';
const validModes: readonly string[] = ['off', 'spotlight', 'magnifier', 'live'];
const safeMode = (mode: unknown): PresentMode =>
  typeof mode === 'string' && validModes.includes(mode) ? mode as PresentMode : 'off';

interface Props {
  device: string;
  running: boolean;
  customPath?: string;
  onRecenter: (device: string) => Promise<void>;
}

/** MirroPresent is opt-in, only for an already-running Windows framed phone.
 * Spotlight and snapshot magnifier temporarily take pointer input over the
 * phone. A neutral 'off' restores the original scrcpy mouse/touch routing.
 * The native daemon, not this React component, owns the mode across tabs.
 */
export default function MirroPresentPanel({device,running,customPath,onRecenter}: Props) {
  const {locale} = useI18n();
  const zh = locale.startsWith('zh');
  const [mode,setMode] = useState<PresentMode>('off');
  const [pending,setPending] = useState(false);
  const [refreshing,setRefreshing] = useState(false);
  const [message,setMessage] = useState<string|null>(null);
  const [error,setError] = useState<string|null>(null);
  const [snapshotReady,setSnapshotReady] = useState(false);
  const generation = useRef(0);
  const locked = useRef(false);

  useEffect(() => {
    let active = true;
    const token = ++generation.current;
    setMode('off');
    setSnapshotReady(false);
    setError(null);
    setMessage(null);
    if (device && running) {
      void invoke<string>('presentation_get_mode',{device}).then(value=>{
        if (active && token === generation.current) setMode(safeMode(value));
      }).catch(() => {/* No Windows phone frame: controls remain disabled. */});
    }
    const unlisten = listen<{device:string,mode:string}>(
      'mirroleap-present-mode',({payload})=>{
        if (!active || payload?.device !== device) return;
        const incoming = safeMode(payload.mode);
        setMode(incoming);
        if (incoming !== 'magnifier') setSnapshotReady(false);
        if (incoming === 'off') setMessage(null);
      });
    return () => {
      active = false;
      generation.current++;
      void unlisten.then(dispose=>dispose());
    };
  },[device,running]);

  const switchMode = async (requested: PresentMode) => {
    if (!device || !running || locked.current) return;
    locked.current = true;
    const token = generation.current;
    setPending(true);
    setError(null);
    setMessage(null);
    try {
      const response = await invoke<string>('presentation_set_mode',{device,mode:requested});
      if (token !== generation.current) return;
      const next = safeMode(response);
      setMode(next);
      setSnapshotReady(false);
      setMessage(next === 'off' ?
        (zh ? '已退出演示，恢复手机触控。' : 'Presentation ended. Phone input restored.') :
        next === 'spotlight' ?
          (zh ? '聚光灯已开启。把鼠标移到投屏画面，突出讲解区域。' :
            'Spotlight ready. Move your mouse over the phone to focus an area.') :
          (zh ? '定格放大镜已开启。先点“刷新放大快照”取得手机画面。' :
            'Snapshot magnifier ready. Refresh a still frame to enable zoom.'));
    } catch(reason) {
      if (token === generation.current) setError(String(reason));
    } finally {
      locked.current = false;
      if (token === generation.current) setPending(false);
    }
  };

  const refreshSnapshot = async () => {
    if (!device || !running || mode !== 'magnifier' || locked.current) return;
    locked.current = true;
    const token = generation.current;
    setRefreshing(true);
    setError(null);
    setMessage(null);
    try {
      await invoke('presentation_refresh_snapshot',{device,customPath});
      if (token !== generation.current) return;
      setSnapshotReady(true);
      setMessage(zh ? '已刷新手机定格画面。此放大镜不是实时视频，画面变化后请再次刷新。' :
        'Still frame refreshed. This is not live video; refresh after the phone screen changes.');
    } catch(reason) {
      if (token === generation.current) {
        setSnapshotReady(false);
        setError(String(reason));
      }
    } finally {
      locked.current = false;
      if (token === generation.current) setRefreshing(false);
    }
  };

  const recenter = async () => {
    if (!running || !device || locked.current) return;
    const token = generation.current;
    setError(null);
    try {
      await onRecenter(device);
      if (token === generation.current) setMessage(zh ?
        '已请求将手机投屏窗口移回主屏幕中央。' : 'Requested centering the phone window on the primary screen.');
    } catch(reason) {
      if (token === generation.current) setError(String(reason));
    }
  };

  const disabled = !device || !running || pending || refreshing;
  return <section className="ml-card flex flex-col gap-3" aria-label="MirroPresent">
    <div className="flex flex-wrap justify-between gap-2 items-center">
      <h2 className="flex items-center gap-2 font-bold text-sm">
        <Focus size={18} className="ml-primary"/>MirroPresent
      </h2>
      <span className="ml-muted text-xs">{zh ? '专业演示模式' : 'Presentation tools'}</span>
    </div>
    <p className="ml-muted text-xs leading-relaxed">
      {zh ?
        '聚光灯、定格放大镜与右侧悬浮栏的 MirroCreator 创作者工具。' :
        'Present with spotlight and a still-frame magnifier.'}
    </p>
    <div className="flex flex-wrap gap-2">
      <button type="button" aria-label={zh ? '开启聚光灯' : 'Enable spotlight'}
        disabled={disabled} onClick={()=>void switchMode('spotlight')}
        aria-pressed={mode==='spotlight'}
        className={'ml-secondary-action inline-flex gap-2 items-center px-3 ' + (mode==='spotlight' ? 'ring-2 ring-primary/60' : '')}>
        <Eye size={16}/>{zh ? '聚光灯' : 'Spotlight'}
      </button>
      <button type="button" aria-label={zh ? '开启定格放大镜' : 'Enable snapshot magnifier'}
        disabled={disabled} onClick={()=>void switchMode('magnifier')}
        aria-pressed={mode==='magnifier'}
        className={'ml-secondary-action inline-flex gap-2 items-center px-3 ' + (mode==='magnifier' ? 'ring-2 ring-primary/60' : '')}>
        <ScanSearch size={16}/>{zh ? '定格放大镜' : 'Snapshot magnifier'}
      </button>
      <button type="button" aria-label={zh ? '退出演示并恢复手机触控' : 'Exit presentation and restore touch'}
        disabled={disabled || mode==='off'} onClick={()=>void switchMode('off')}
        className="ml-secondary-action inline-flex gap-2 items-center px-3">
        <XCircle size={16}/>{zh ? '退出演示' : 'Exit presenter'}
      </button>
    </div>
    <div className="flex flex-wrap gap-2">
      <button type="button" aria-label={zh ? '刷新放大快照' : 'Refresh magnifier snapshot'}
        disabled={disabled || mode!=='magnifier'}
        onClick={()=>void refreshSnapshot()}
        className="ml-secondary-action inline-flex gap-2 items-center px-3">
        <RefreshCw size={16} className={refreshing ? 'animate-spin' : ''}/>
        {refreshing ? (zh ? '正在读取手机快照…' : 'Capturing phone still…') :
          (zh ? '刷新放大快照' : 'Refresh still frame')}
      </button>
      <button type="button" disabled={disabled} onClick={()=>void recenter()}
        aria-label={zh ? '居中演示窗口' : 'Center presentation window'}
        className="ml-secondary-action inline-flex gap-2 items-center px-3">
        <Move size={16}/>{zh ? '居中演示窗口' : 'Center phone window'}
      </button>
    </div>
    {mode==='magnifier' && <p className="ml-muted text-xs">
      <Maximize size={14} className="inline mr-1"/>
      {snapshotReady ?
        (zh ? '已加载静态手机截图。画面内滚轮调整圆形大小，Ctrl+滚轮调整放大倍率；画面变化后请刷新。' :
          'Still image loaded. Wheel changes lens size; Ctrl+wheel adjusts magnification. Refresh after changes.') :
        (zh ? '尚未加载截图。请点击刷新放大快照。' : 'No still frame loaded. Refresh before zooming.')}
    </p>}
    {mode==='live' && <p role="note" className="ml-muted text-xs">
      {zh
        ? '连续放大正在使用新的 ADB 截图更新画面（通常只有每秒约 1–3 帧，可能影响无线投屏流畅度）。可通过悬浮栏或退出演示停止采样。'
        : 'Continuous lens uses fresh ADB screenshots at roughly 1–3 fps and may impact Wi-Fi mirroring. Exit presenter to stop sampling.'}
    </p>}
    {mode==='spotlight' && <p role="note" className="ml-muted text-xs">
      {zh ? '在手机画面内滚动鼠标滚轮即可放大或缩小聚光灯范围。' :
        'Scroll the mouse wheel over the phone to resize the spotlight.'}
    </p>}
    {!running && <p role="note" className="text-xs text-amber-600">
      {zh ? '请先连接手机并启动带真机边框的 Windows 投屏，再使用演示模式。' :
        'Start a Windows phone mirror with the device frame enabled before presenting.'}
    </p>}
    {message && <p role="status" className="text-xs text-emerald-600">{message}</p>}
    {error && <p role="alert" className="text-xs text-red-600 break-words">
      {zh ? '演示操作失败：' : 'Presentation action failed: '}{error}
    </p>}
    <details className="ml-settings-help-details">
      <summary>{zh ? '使用说明与安全提示' : 'How it works & safety'}</summary>
      <p>
        {zh ?
          '聚光灯或放大镜开启时暂时接管手机触控；退出演示恢复。创作者的遮挡框只覆盖可见图层，不保护 scrcpy 原始录像、OBS 单窗口捕捉或普通截图，请勿据此处理敏感信息。连续放大采用低帧率 ADB 新截图，不是 60FPS 视频放大。右侧悬浮栏“一键恢复”可紧急退出并清空标注。' :
          'Presenter modes temporarily intercept taps; exit to restore touch. Creator masks only cover the visible overlay, NOT raw scrcpy recordings, OBS single-window capture or unannotated screenshots. Do not rely on masks to hide sensitive content. Continuous lens uses low-FPS fresh ADB screenshots, not 60fps video. One-click restore exits and clears annotations.'}
      </p>
    </details>
  </section>;
}
