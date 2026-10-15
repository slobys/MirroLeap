import { useState } from 'react';
import { Camera, Image as ImageIcon, LoaderCircle, PenLine } from 'lucide-react';
import { invoke } from '@tauri-apps/api/core';
import { useI18n } from '../i18n';

type Props = { device: string; customPath?: string; };

export function screenshotErrorMessage(reason: unknown): string {
  return reason instanceof Error ? reason.message : String(reason);
}

/** MirroCapture: two explicit exports, under Settings. The native compositor
 * requests the *current* transparent paint canvas from the selected phone.
 * Never pretend pen marks were exported if the overlay is unavailable.
 */
export default function ScreenshotTools({device, customPath}: Props) {
  const {locale} = useI18n();
  const zh = locale.startsWith('zh');
  const [pending, setPending] = useState<'original' | 'annotated' | null>(null);
  const [saved, setSaved] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);

  const capture = async (mode: 'original' | 'annotated') => {
    if (pending || !device) return;
    setPending(mode);
    setSaved(null);
    setError(null);
    try {
      const path = await invoke<string>(mode === 'annotated'
          ? 'save_annotated_screenshot' : 'save_device_screenshot', {
        device, customPath,
      });
      setSaved(path);
    } catch (reason) {
      setError(screenshotErrorMessage(reason));
    } finally {
      setPending(null);
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
          ? '将选中手机的画面保存为 PNG 到电脑“下载”文件夹。可选择原始截图或包含 MirroLeap 画笔的合成截图。'
          : 'Save the selected Android screen as a PNG in Downloads, with or without your MirroLeap pen annotations.'}
      </p>
      <p className="ml-muted text-[11px]">
        {zh
          ? '带批注截图需要先开启投屏并画出至少一笔。只保存手机画面和画笔，不包含手机外框、悬浮工具栏或电脑桌面。'
          : 'Annotated capture requires an active mirrored phone and at least one pen stroke. The phone bezel, floating toolbar and desktop are excluded.'}
      </p>
      <div className="flex flex-wrap gap-2">
        <button
          type="button"
          onClick={() => void capture('original')}
          disabled={!device || Boolean(pending)}
          aria-label={zh ? '保存手机原始截图' : 'Save phone screenshot'}
          className="ml-secondary-action inline-flex items-center justify-center gap-2 px-4">
          {pending === 'original' ? <LoaderCircle size={16} className="animate-spin"/> : <Camera size={16}/>}
          {pending === 'original' ? (zh ? '正在截取…' : 'Capturing…') : (zh ? '保存原始截图' : 'Save original screenshot')}
        </button>
        <button
          type="button"
          onClick={() => void capture('annotated')}
          disabled={!device || Boolean(pending)}
          aria-label={zh ? '保存含画笔截图' : 'Save annotated screenshot'}
          className="ml-secondary-action inline-flex items-center justify-center gap-2 px-4">
          {pending === 'annotated' ? <LoaderCircle size={16} className="animate-spin"/> : <PenLine size={16}/>}
          {pending === 'annotated' ? (zh ? '正在合成…' : 'Combining…') : (zh ? '保存含画笔截图' : 'Save annotated screenshot')}
        </button>
      </div>
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
