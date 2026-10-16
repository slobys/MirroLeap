import { useEffect, useRef, useState } from 'react';
import { Camera, Copy, Image as ImageIcon, LoaderCircle, PenLine } from 'lucide-react';
import { invoke } from '@tauri-apps/api/core';
import { Image } from '@tauri-apps/api/image';
import { writeImage } from '@tauri-apps/plugin-clipboard-manager';
import { useI18n } from '../i18n';

type Props = { device: string; customPath?: string; };

export function screenshotErrorMessage(reason: unknown): string {
  return reason instanceof Error ? reason.message : String(reason);
}

/** A bounded PNG data URL supplied by the main-window-only Rust preview API.
 * Clipboard writes are triggered exclusively by the Copy button, not by
 * capturing or loading the phone screen preview. */
export function pngDataUrlBytes(dataUrl: string): Uint8Array<ArrayBuffer> {
  const prefix = 'data:image/png;base64,';
  if (!dataUrl.startsWith(prefix) || dataUrl.length > 18_000_000) {
    throw Error('Invalid or oversized PNG preview');
  }
  const decoded = atob(dataUrl.slice(prefix.length));
  const bytes = new Uint8Array(new ArrayBuffer(decoded.length));
  for (let i = 0; i < decoded.length; i++) bytes[i] = decoded.charCodeAt(i);
  if (bytes.length < 24 || bytes[0] !== 137 || bytes[1] !== 80 || bytes[2] !== 78 || bytes[3] !== 71) {
    throw Error('Preview did not contain PNG image bytes');
  }
  return bytes;
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
  const [previewUrl, setPreviewUrl] = useState<string | null>(null);
  const [previewWarning, setPreviewWarning] = useState<string | null>(null);
  const [copyMessage, setCopyMessage] = useState<string | null>(null);
  const [copying, setCopying] = useState(false);
  const generation = useRef(0);

  // A saved phone screenshot may contain personal data. Never leave another
  // device's preview visible after the user switches selected phones.
  useEffect(() => {
    generation.current += 1;
    setPreviewUrl(null);
    setSaved(null);
    setError(null);
    setPreviewWarning(null);
    setCopyMessage(null);
    setPending(null);
  }, [device]);

  const capture = async (mode: 'original' | 'annotated') => {
    if (pending || !device) return;
    const request = ++generation.current;
    setPending(mode);
    setSaved(null);
    setError(null);
    setPreviewUrl(null);
    setPreviewWarning(null);
    setCopyMessage(null);
    try {
      const path = await invoke<string>(mode === 'annotated'
          ? 'save_annotated_screenshot' : 'save_device_screenshot', {
        device, customPath,
      });
      if (request !== generation.current) return;
      setSaved(path);
      try {
        const png = await invoke<string>('preview_saved_screenshot', { path });
        if (request === generation.current && png.startsWith('data:image/png;base64,')) {
          setPreviewUrl(png);
        }
      } catch (previewError) {
        if (request === generation.current) {
          setPreviewWarning(screenshotErrorMessage(previewError));
        }
      }
    } catch (reason) {
      if (request === generation.current) setError(screenshotErrorMessage(reason));
    } finally {
      if (request === generation.current) setPending(null);
    }
  };

  const copyPreview = async () => {
    if (!previewUrl || copying || pending) return;
    const request = generation.current;
    setCopying(true);
    setCopyMessage(null);
    try {
      const bytes = pngDataUrlBytes(previewUrl);
      const image = await Image.fromBytes(bytes);
      try {
        if (request !== generation.current) return;
        await writeImage(image);
      } finally {
        await image.close();
      }
      if (request === generation.current) {
        setCopyMessage(zh ? '已复制图片，可直接粘贴到电脑应用。' : 'Copied image. Paste into any desktop app.');
      }
    } catch (reason) {
      if (request === generation.current) {
        setCopyMessage((zh ? '复制失败：' : 'Clipboard copy failed: ') + screenshotErrorMessage(reason));
      }
    } finally {
      setCopying(false);
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
      {previewUrl && <div className="flex flex-col gap-3" aria-label={zh ? '截图预览' : 'Screenshot preview'}>
        <div className="rounded-xl border border-[var(--border-base)] bg-[var(--bg-elevated)] p-2 flex justify-center">
          <img src={previewUrl}
            className="max-h-72 max-w-full object-contain rounded-lg"
            alt={zh ? '最近一次保存的手机截图预览' : 'Preview of the most recently saved phone screenshot'} />
        </div>
        <button type="button"
          onClick={() => void copyPreview()}
          disabled={copying || Boolean(pending)}
          aria-label={zh ? '复制截图到剪贴板' : 'Copy screenshot to clipboard'}
          className="ml-secondary-action px-4 inline-flex items-center self-start gap-2">
          {copying ? <LoaderCircle size={16} className="animate-spin"/> : <Copy size={16}/>}
          {zh ? '复制图片到剪贴板' : 'Copy image to clipboard'}
        </button>
      </div>}
      {previewWarning && <p className="ml-muted text-xs" role="note">
        {zh ? '截图已保存，但预览不可用：' : 'Screenshot saved but preview unavailable: '}{previewWarning}
      </p>}
      {copyMessage && <p role="status" className="ml-muted text-xs">{copyMessage}</p>}
      {error && <p role="alert" className="text-xs text-red-600 break-all">
        {zh ? '截图失败：' : 'Capture failed: '}{error}
      </p>}
    </section>
  );
}
