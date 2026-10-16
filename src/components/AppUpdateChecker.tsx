import { useEffect, useRef, useState } from 'react';
import { CheckCircle2, Download, ExternalLink, LoaderCircle, RefreshCw, ShieldCheck } from 'lucide-react';
import { invoke } from '@tauri-apps/api/core';
import { useI18n } from '../i18n';
import { explainUpdateError, trustedReleaseDownload, trustedReleaseUrl,
  type AppUpdateInfo } from '../utils/appUpdate';

interface Props { version: string; }

/**
 * Intentionally explicit/read-only. No polling, no installer execution and no
 * elevated privileges: code signing is not configured for Windows installer
 * updates. App version != scrcpy core version; they update independently.
 */
export default function AppUpdateChecker({version}: Props) {
  const {locale} = useI18n();
  const zh = locale.startsWith('zh');
  const [pending, setPending] = useState(false);
  const [info, setInfo] = useState<AppUpdateInfo | null>(null);
  const [error, setError] = useState<string | null>(null);
  const generation = useRef(0);
  const lock = useRef(false);

  useEffect(() => {
    // Settings tab unmounts on navigation. Old IPC responses may never update
    // a newly mounted page or overwrite another installed app's version.
    return () => { generation.current++; };
  }, []);

  const check = async () => {
    if (lock.current) return;
    const request = ++generation.current;
    lock.current = true;
    setPending(true);
    setInfo(null);
    setError(null);
    try {
      const result = await invoke<AppUpdateInfo>('check_mirroleap_update');
      if (request !== generation.current) return;
      if (!trustedReleaseUrl(result) ||
          !['update_available', 'up_to_date', 'ahead'].includes(result.status)) {
        setError(explainUpdateError('invalid_release', zh));
        return;
      }
      setInfo(result);
    } catch (reason) {
      if (request === generation.current) setError(explainUpdateError(reason, zh));
    } finally {
      lock.current = false;
      if (request === generation.current) setPending(false);
    }
  };

  const officialPage = 'https://github.com/slobys/MirroLeap/releases';
  const releasePage = info && trustedReleaseUrl(info);
  const downloads = info?.downloads.filter(asset => trustedReleaseDownload(info, asset)) || [];
  const updateAvailable = info?.status === 'update_available';

  return <section className="ml-card flex flex-col gap-3" aria-label={zh ? 'MirroLeap 软件更新' : 'MirroLeap software updates'}>
    <div className="flex flex-wrap items-center justify-between gap-3">
      <div className="flex items-center gap-2">
        <RefreshCw size={18} className="ml-primary"/>
        <h2 className="font-bold text-sm">{zh ? 'MirroLeap 软件更新' : 'MirroLeap software updates'}</h2>
      </div>
      <span className="ml-muted text-xs">{zh ? '当前安装版本' : 'Installed version'}：v{version}</span>
    </div>
    <p className="ml-muted text-xs leading-relaxed">
      {zh
        ? '手动读取 GitHub 正式 Release 版本，不检查测试版，不会自动下载或执行安装程序。scrcpy 投屏引擎由上方独立管理。'
        : 'Manually check GitHub stable Releases only. No preview builds, automatic installer downloads or execution. The scrcpy engine is managed separately above.'}
    </p>
    <div className="flex flex-wrap items-center gap-3">
      <button type="button" onClick={() => void check()} disabled={pending}
        aria-label={zh ? '检查 MirroLeap 软件更新' : 'Check for MirroLeap software updates'}
        className="ml-secondary-action inline-flex items-center gap-2 px-4">
        {pending ? <LoaderCircle size={16} className="animate-spin"/> : <RefreshCw size={16}/>}
        {pending ? (zh ? '正在连接 GitHub…' : 'Checking GitHub…') :
          (zh ? '检查软件更新' : 'Check for updates')}
      </button>
      <a href={officialPage} target="_blank" rel="noopener noreferrer"
        className="ml-footer-link inline-flex items-center gap-1">
        <ExternalLink size={13}/>
        {zh ? '官方发布页面' : 'Official releases'}
      </a>
    </div>
    {error && <p role="alert" className="text-xs text-amber-600 leading-relaxed">{error}</p>}
    {info && <div role="status" aria-live="polite" className="flex flex-col gap-3 rounded-lg border border-[var(--border-base)] p-3">
      <div className="flex items-start gap-2">
        {updateAvailable ? <Download size={18} className="ml-primary shrink-0"/> :
          <CheckCircle2 size={18} className="text-emerald-600 shrink-0"/>}
        <div className="flex flex-col gap-1">
          <p className="font-semibold text-sm">
            {updateAvailable ?
              (zh ? `发现正式版 v${info.latestVersion}` : `Stable update v${info.latestVersion} available`) :
              info.status === 'ahead' ?
                (zh ? `当前构建高于最新正式版 v${info.latestVersion}` : `Installed build is newer than stable v${info.latestVersion}`) :
                (zh ? `当前已是最新正式版 v${info.latestVersion}` : `Up to date: v${info.latestVersion}`)}
          </p>
          {info.publishedAt && <span className="ml-muted text-xs">{zh ? '发布时间' : 'Published'}：{info.publishedAt.slice(0, 10)}</span>}
        </div>
      </div>
      {info.changelog && <div className="flex flex-col gap-1">
        <p className="text-xs font-medium">{zh ? '版本更新说明' : 'Release notes'}</p>
        <pre className="ml-muted max-h-40 overflow-y-auto whitespace-pre-wrap break-words font-sans text-xs leading-relaxed">
          {info.changelog}
        </pre>
      </div>}
      {updateAvailable && downloads.length > 0 && <div className="flex flex-col gap-2">
        <span className="text-xs font-medium">{zh ? '官方下载文件' : 'Official downloads'}</span>
        <div className="flex flex-wrap gap-3">
          {downloads.map(asset => <a key={asset.kind}
            href={trustedReleaseDownload(info, asset) || undefined}
            target="_blank" rel="noopener noreferrer"
            className="ml-footer-link inline-flex items-center gap-1 text-xs">
            <Download size={13}/>
            {asset.kind === 'portable' ? (zh ? '免安装 ZIP' : 'Portable ZIP') :
              asset.kind === 'exe' ? 'EXE' : asset.kind === 'msi' ? 'MSI' :
                (zh ? 'SHA-256 校验文件' : 'SHA-256 manifest')}
          </a>)}
        </div>
        <p className="ml-muted text-[11px]">
          {zh ? '下载前请在官方页面核对文件及 SHA-256，下载后由你自行决定是否安装。' :
            'Verify the official file and SHA-256 checksum before manually installing.'}
        </p>
      </div>}
      {releasePage && <a href={releasePage} target="_blank" rel="noopener noreferrer"
        className="ml-footer-link inline-flex items-center self-start gap-1">
        <ExternalLink size={13}/>{zh ? '查看该正式版本的 GitHub 页面' : 'View this stable release'}
      </a>}
    </div>}
    <p className="ml-muted flex items-start gap-1.5 text-[11px] leading-relaxed">
      <ShieldCheck size={14} className="shrink-0"/>
      {zh ? '目前尚未配置可信 Windows 代码签名；本功能只做版本检查与官方链接引导，不会静默安装，也不要求关闭 Windows 安全防护。' :
        'Trusted Windows code signing is not configured. This feature never installs silently or asks you to disable system protection.'}
    </p>
  </section>;
}
