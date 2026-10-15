import { useState } from 'react';
import { Camera, Image as ImageIcon, LoaderCircle } from 'lucide-react';
import { invoke } from '@tauri-apps/api/core';
import { useI18n } from '../i18n';

type Props = { device: string; customPath?: string; };

export function screenshotErrorMessage(reason: unknown): string {
  return reason instanceof Error ? reason.message : String(reason);
}

/** MirroCapture: a single opt-in native Android screenshot, under Settings.
 * Keeping this out of the minimalist home avoids adding rarely-used buttons.
 * Only the original Android pixels are saved in v1 (no overlay annotations).
 */
export default function ScreenshotTools({device, customPath}: Props) {
  const {locale} = useI18n();
  const zh = locale.startsWith('zh');
  const [pending, setPending] = useState(false);
  const [saved, setSaved] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);

  const capture = async () => {
    if (pending || !device) return;
    setPending(true);
    setSaved(null);
    setError(null);
    try {
      const path = await invoke<string>('save_device_screenshot', {
        device, customPath,
      });
      setSaved(path);
    } catch (reason) {
      setError(screenshotErrorMessage(reason));
    } finally {
      setPending(false);
    }
  };
  return (
    <section className="ml-card flex flex-col gap-3" aria-label={zh ? 'MirroCapture 截图' : 'MirroCapture screenshot'}>
      <div className="flex items-center gap-2">
        <ImageIcon size={19} className="ml-primary"/>
        <h2 className="font-bold text-sm">MirroCapture</h2>
      </div>
      <p className="ml-muted text-xs leading-relaxed">
        {zh
          ? '把已选手机的原始画面保存为 PNG 到电脑“下载”文件夹。只在点击时截图，不上传云端。'
          : 'Save the selected phone’s original screen as PNG in Downloads. Captures only when clicked, never uploads.'}
      </p>
      <p className="ml-muted text-[11px]">
        {zh
          ? '目前不包含 MirroLeap 画笔批注；批注与截图合成导出将在后续版本提供。'
          : 'Pen annotations are not included yet; merged annotation export is planned separately.'}
      </p>
      <button
        type="button"
        onClick={() => void capture()}
        disabled={!device || pending}
        aria-label={zh ? '保存手机截图' : 'Save phone screenshot'}
        className="ml-secondary-action inline-flex items-center self-start justify-center gap-2 px-4">
        {pending ? <LoaderCircle size={16} className="animate-spin"/> : <Camera size={16}/>}
        {pending ? (zh ? '正在截取…' : 'Capturing…') : (zh ? '保存手机截图' : 'Save phone screenshot')}
      </button>
      {!device && <p role="note" className="ml-muted text-xs">
        {zh ? '请先在首页连接并选择一台手机。' : 'Connect and select a device from Home first.'}
      </p>}
      {saved && <p role="status" className="text-xs text-emerald-600 break-all">
        {zh ? '已保存：' : 'Saved: '}{saved}
      </p>}
      {error && <p role="alert" className="text-xs text-red-600 break-all">
        {zh ? '截图失败：' : 'Capture failed: '}{error}
      </p>}
    </section>
  );
}
