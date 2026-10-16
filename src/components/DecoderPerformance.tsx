import { Activity, Cpu, Gauge, ShieldCheck } from 'lucide-react';
import type { ScrcpyConfig, VideoDecoderSupport } from '../hooks/useScrcpy';
import { useI18n } from '../i18n';

interface Props {
  config: ScrcpyConfig;
  setConfig: (value: ScrcpyConfig) => void;
  support: VideoDecoderSupport | null;
  observedMode?: string;
  running: boolean;
}

const DECODER_LABELS: Record<string, [string, string]> = {
  auto: ['自动（推荐：硬件失败时可回退软件）', 'Auto (recommended; hardware → software fallback)'],
  disabled: ['仅软件解码（兼容性优先）', 'Software decoding only (compatibility)'],
  d3d11va: ['Windows D3D11 GPU（高级）', 'Windows D3D11 GPU (advanced)'],
  vaapi: ['Linux VA-API GPU（高级）', 'Linux VA-API GPU (advanced)'],
  videotoolbox: ['macOS VideoToolbox（高级）', 'macOS VideoToolbox (advanced)'],
};

/** Host decoding is neither the phone's codec/encoder nor the SDL renderer.
 * All settings apply on NEXT launch; this panel never restarts a live mirror.
 */
export default function DecoderPerformance({config, setConfig, support, observedMode, running}: Props) {
  const {locale} = useI18n();
  const zh = locale.startsWith('zh');
  const supported = Boolean(support?.supportsHwdec);
  const modes = supported ? support?.modes || [] : [];
  const requested = config.hardwareDecoder || 'auto';
  const selected = modes.includes(requested) ? requested : 'auto';
  const actual = observedMode && !['starting', 'retrying', 'unknown'].includes(observedMode)
    ? observedMode : null;
  const h265OldWindows = config.codec === 'h265' && support?.hostOs === 'windows' &&
    support.scrcpyVersion === '5.0.0';

  return <section className="ml-card flex flex-col gap-3" aria-label={zh ? 'GPU 解码与性能' : 'GPU decoding and performance'}>
    <div className="flex flex-wrap items-center justify-between gap-2">
      <h2 className="flex items-center gap-2 font-bold text-sm">
        <Cpu size={18} className="ml-primary"/>{zh ? 'GPU 解码与性能' : 'GPU decoding & performance'}
      </h2>
      <span className="ml-muted text-xs">
        {zh ? '投屏核心' : 'scrcpy core'}：
        {support?.scrcpyVersion ? `v${support.scrcpyVersion}` : (zh ? '未检测' : 'Not detected')}
      </span>
    </div>
    <p className="ml-muted text-xs leading-relaxed">
      {zh
        ? '电脑端视频解码；建议优先使用自动模式。'
        : 'Desktop video decoding. Auto is recommended.'}
    </p>
    <label htmlFor="ml-hwdec" className="font-medium text-xs">
      {zh ? '电脑端视频解码方式' : 'Host video decoder'}
    </label>
    <select id="ml-hwdec" className="ml-select"
      value={selected}
      disabled={!supported}
      onChange={(event) => setConfig({
        ...config,
        hardwareDecoder: event.target.value as ScrcpyConfig['hardwareDecoder'],
      })}>
      <option value="auto">{zh ? DECODER_LABELS.auto[0] : DECODER_LABELS.auto[1]}</option>
      {modes.filter(mode => mode !== 'auto').map(mode => (
        <option key={mode} value={mode}>
          {zh ? (DECODER_LABELS[mode]?.[0] || mode) : (DECODER_LABELS[mode]?.[1] || mode)}
        </option>
      ))}
    </select>
    {!supported && <p role="note" className="text-xs text-amber-600">
      {zh
        ? '当前 scrcpy 核心尚未确认支持 --hwdec（需要 5.0 或更高版本及对应命令）。软件会继续沿用原来的解码方式，不会强制传入不兼容参数。'
        : 'The installed scrcpy core has not confirmed --hwdec support (requires 5.0+). Mirroring keeps its existing decoder flags.'}
    </p>}
    {supported && <p className="ml-muted text-xs">
      {zh
        ? '检测到 CLI 参数支持不代表 GPU 驱动一定能使用。强制硬解失败时会尝试一次默认自动模式。'
        : 'CLI support does not prove that your GPU driver can decode this stream. A failed forced GPU decoder retries once in auto mode.'}
    </p>}
    {h265OldWindows && <p role="note" className="text-xs text-amber-600">
      {zh ? 'scrcpy 5.0.0 的部分 Windows H.265 硬解存在兼容性问题，建议使用 5.0.1 或更新版本。'
        : 'Some Windows H.265 hardware decode issues in scrcpy 5.0.0 were fixed in 5.0.1. Consider updating the engine.'}
    </p>}
    {requested !== 'auto' && !modes.includes(requested) && <p role="note" className="text-xs text-amber-600">
      {zh ? '之前保存的解码选择在当前核心不可用，启动时将使用自动/默认解码。'
        : 'Your saved decoder preference is unavailable in this core and will fall back to the default.'}
    </p>}
    <label className="inline-flex items-center gap-2 text-xs">
      <input type="checkbox" className="accent-primary"
        checked={Boolean(config.printFps) && Boolean(support?.supportsPrintFps)}
        disabled={!support?.supportsPrintFps}
        onChange={(event) => setConfig({...config, printFps: event.target.checked})}/>
      <Gauge size={15} className="ml-primary"/>
      {zh ? '在运行日志输出实际帧率（scrcpy --print-fps）' : 'Log actual FPS (scrcpy --print-fps)'}
    </label>
    {running && <p className="ml-muted text-xs">
      {zh ? '解码参数和 FPS 日志开关将在下次投屏启动时生效，不会打断当前画面。'
        : 'Decoder and FPS logging preferences apply to your next mirroring session without interrupting this one.'}
    </p>}
    <div className="flex items-center gap-2 text-xs" role="status">
      <Activity size={15} className={actual ? 'text-emerald-600' : 'ml-primary'}/>
      {zh ? '实际解码情况（来自 scrcpy 日志）：' : 'Observed decoder (scrcpy log): '}
      <span className="font-semibold">
        {actual || (observedMode === 'retrying'
          ? (zh ? '已触发兼容回退' : 'Retrying compatible decoder')
          : observedMode === 'starting'
            ? (zh ? '等待 scrcpy 输出' : 'Waiting for scrcpy log')
            : (zh ? '尚无记录' : 'Not yet reported'))}
      </span>
    </div>
    <details className="ml-settings-help-details">
      <summary className="inline-flex items-center gap-2">
        <ShieldCheck size={14}/>{zh ? '解码说明' : 'Decoder notes'}
      </summary>
      <p>{zh ? 'GPU 模式不会自动更新你的 scrcpy 核心，也不能单凭选项预测 CPU 降幅或实际延迟。请比较实际帧率、掉帧及画面流畅程度。'
        : 'GPU selection never upgrades your core and does not predict CPU or latency gains. Compare logged FPS, dropped frames and real smoothness.'}</p>
    </details>
  </section>;
}
