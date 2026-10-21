import {useEffect, useState} from 'react';
import {Activity, Gauge, Wifi} from 'lucide-react';
import type {MirrorPerformance, ScrcpyConfig, VideoDecoderSupport} from '../hooks/useScrcpy';
import { AUTO_QUALITY_SIZES } from '../utils/autoQuality';

type Props = {
    locale: string;
    device: string;
    running: boolean;
    config: ScrcpyConfig;
    setConfig: (next: ScrcpyConfig) => void;
    performance?: MirrorPerformance;
    decoder?: string;
    support: VideoDecoderSupport | null;
};

export default function LivePerformance({locale, device, running, config, setConfig, performance, decoder, support}: Props) {
    const zh = locale.startsWith('zh');
    const autoResolutionSupported = AUTO_QUALITY_SIZES.includes(
        config.res as typeof AUTO_QUALITY_SIZES[number]);
    const [now, setNow] = useState(() => Date.now());
    useEffect(() => {
        if (!running) return;
        const tick = setInterval(() => setNow(Date.now()), 5000);
        return () => clearInterval(tick);
    }, [running]);
    const available = running && performance && now - performance.observedAt < 12_000;
    const fps = available ? performance.fps.toFixed(1) : '—';
    const samples = available ? performance.samples.slice(-20) : [];
    const decoderName = running && decoder === 'unreported'
        ? (zh ? '核心未报告（无法确认）' : 'Not reported (unconfirmed)')
        : running && decoder && !['unknown','starting','retrying'].includes(decoder)
            ? decoder : '—';

    return <section className="ml-card flex flex-col gap-3" aria-label={zh ? '实时性能监控' : 'Live performance monitor'}>
        <h2 className="flex gap-2 items-center font-bold text-sm">
            <Activity size={18} className="ml-primary"/>
            {zh ? '实时性能监控' : 'Live performance monitor'}
        </h2>
        <div className="grid grid-cols-2 sm:grid-cols-4 gap-2 text-center" data-testid="live-performance">
            <div className="rounded-lg border border-zinc-500/20 px-2 py-3">
                <p className="ml-muted text-xs">{zh ? '实测 FPS' : 'Measured FPS'}</p>
                <p className="text-lg font-semibold tabular-nums" data-testid="measured-fps">{fps}</p>
            </div>
            <div className="rounded-lg border border-zinc-500/20 px-2 py-3">
                <p className="ml-muted text-xs">{zh ? '设定码率' : 'Target bitrate'}</p>
                <p className="text-lg font-semibold tabular-nums">{config.bitrate ?? 8} <span className="text-xs">Mbps</span></p>
            </div>
            <div className="rounded-lg border border-zinc-500/20 px-2 py-3">
                <p className="ml-muted text-xs">{zh ? '本次跳帧' : 'Skipped frames'}</p>
                <p className="text-lg font-semibold tabular-nums" data-testid="measured-skipped">{available && performance.skipped !== undefined ? performance.skipped : '—'}</p>
            </div>
            <div className="rounded-lg border border-zinc-500/20 px-2 py-3">
                <p className="ml-muted text-xs">{zh ? '实际解码方式' : 'Observed decoder'}</p>
                <p className="text-xs font-semibold break-words mt-2">{decoderName}</p>
            </div>
        </div>
        {samples.length > 1 && <div className="flex items-end gap-0.5 h-11" aria-label={zh ? '近期 FPS 趋势' : 'Recent measured FPS trend'} role="img">
            {samples.map(({fps: value, at}, index) => <span key={at + '-' + index}
                className="bg-primary/75 flex-1 min-w-0 rounded-sm"
                style={{height: Math.max(4, Math.min(100, value / 60 * 100)) + '%'}}/>)}
        </div>}
        {!running ? <p className="ml-muted text-xs">{zh ? '开始投屏后显示实时测量数据。' : 'Start mirroring to collect measurements.'}</p>
            : !available && <p className="ml-muted text-xs">
                {support?.supportsPrintFps && (config.printFps || config.autoQuality)
                    ? (zh ? '等待 scrcpy 输出实测帧率；静止画面可能不会持续产生新帧。' : 'Waiting for measured FPS. A static screen may not produce new frames.')
                    : (zh ? '启用「在运行日志输出实际帧率」并重新开始投屏以获取 FPS。' : 'Enable FPS logging and restart mirroring to collect measurements.')}
            </p>}
        {available && performance.skipped === undefined && (
            <p className="text-xs text-amber-600 dark:text-amber-400" role="note"
                data-testid="skipped-not-reported">
                {zh
                    ? '当前 scrcpy 只提供帧率，没有提供跳帧数；“—”表示未知而不是 0。Auto 不会用缺失的跳帧数据判断是否升降画质。'
                    : 'scrcpy reports FPS but not skipped-frame counts. “—” means unknown, not zero. Auto will not treat missing skip data as evidence to change quality.'}
            </p>
        )}
        <p className="ml-muted text-xs flex gap-1.5 items-start">
            <Gauge size={14} className="shrink-0 mt-0.5"/>
            {zh ? 'FPS 仅来自 scrcpy 实际日志。码率是所选编码上限，跳帧指 scrcpy 渲染时的实际跳过帧数，不代表网络丢包；不显示未经测量的延迟。'
                : 'FPS is read from actual scrcpy output. Bitrate is the configured target, not network throughput. Skipped frames are reported by the scrcpy renderer, not network packet loss. Latency is not estimated.'}
        </p>
        <label className="flex items-center gap-2 text-sm font-medium">
            <input type="checkbox" className="accent-primary" aria-label={zh ? '智能画质 Auto 模式' : 'Smart quality Auto'}
                checked={Boolean(config.autoQuality)}
                disabled={!device || !support?.supportsPrintFps || config.sessionMode !== 'mirror' ||
                    (!autoResolutionSupported && !config.autoQuality)}
                onChange={event => setConfig({...config, autoQuality:event.target.checked,
                    printFps:event.target.checked ? true : config.printFps})}/>
            {zh ? '智能画质 Auto（稳定优先）' : 'Smart quality Auto (stability first)'}
        </label>
        <p className="ml-muted text-xs leading-5" role="note">
            {zh
                ? '需手动开启：仅在持续测得低帧率或高帧率、且非录制状态时保守调整码率或分辨率。每次至少冷却 3 分钟，最高自动升到 2K。实际低帧率也可能来自视频本身，并不能证明 Wi-Fi 变慢。'
                : 'Opt-in only: sustained measured FPS may cautiously change bitrate or resolution when not recording. At least 3 minutes between adjustments; never auto-upgrade past 2K. Low FPS does not necessarily mean weak Wi-Fi.'}
        </p>
        {!autoResolutionSupported && <p className="ml-muted text-xs" role="note"
            data-testid="auto-quality-resolution-note">
            {zh
                ? 'Auto 支持 480P、576P、720P、900P、1080P 和 2K；4K 及其他自定义分辨率保持手动控制。'
                : 'Auto supports 480p, 576p, 720p, 900p, 1080p and 2K. 4K and other custom resolutions remain manual.'}
        </p>}
        {running && config.autoQuality && !performance && <p className="text-xs text-amber-600" role="note">
            {zh ? '若本次投屏未启用 FPS 日志，Auto 将等待下次启动，不会打断正在播放的画面。'
                : 'If FPS logging was not active at launch, Auto waits until the next session without interrupting playback.'}
        </p>}
        {!support?.supportsPrintFps && <p className="ml-muted text-xs">
            {zh ? '当前核心不支持实测 FPS，Auto 保持禁用。' : 'Auto requires an engine that reports measured FPS.'}
        </p>}
        {!device && <p className="ml-muted text-xs flex items-center gap-1"><Wifi size={14}/>{zh ? '请先选择手机' : 'Select a phone first'}</p>}
    </section>;
}
