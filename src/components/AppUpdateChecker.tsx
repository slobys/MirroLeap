import { useEffect, useRef, useState } from 'react';
import { CheckCircle2, Download, ExternalLink, LoaderCircle, RefreshCw, ShieldCheck } from 'lucide-react';
import { invoke } from '@tauri-apps/api/core';
import { check as checkSignedUpdate } from '@tauri-apps/plugin-updater';
import { useI18n } from '../i18n';
import { explainUpdateError, trustedReleaseDownload, trustedReleaseUrl,
  type AppUpdateInfo } from '../utils/appUpdate';

interface Props { version: string; }

/**
 * User-confirmed signed updates. No automatic polling/install. Portable ZIP
 * installs remain manual. The Android scrcpy core updater is independent.
 */
export default function AppUpdateChecker({version}: Props) {
  const {locale} = useI18n();
  const zh = locale.startsWith('zh');
  const [pending, setPending] = useState(false);
  const [info, setInfo] = useState<AppUpdateInfo | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [installationAllowed, setInstallationAllowed] = useState(false);
  const [installing, setInstalling] = useState(false);
  const [progress, setProgress] = useState<{received:number,total:number|null} | null>(null);
  const [installPhase, setInstallPhase] = useState<'downloading'|'installing'|null>(null);
  const generation = useRef(0);
  const lock = useRef(false);
  const installLock = useRef(false);

  useEffect(() => {
    // Settings tab unmounts on navigation. Old IPC responses may never update
    // a newly mounted page or overwrite another installed app's version.
    return () => { generation.current++; };
  }, []);

  const check = async () => {
    if (lock.current || installLock.current) return;
    const request = ++generation.current;
    lock.current = true;
    setPending(true);
    setInfo(null);
    setError(null);
    setInstallationAllowed(false);
    setProgress(null);
    setInstallPhase(null);
    try {
      const result = await invoke<AppUpdateInfo>('check_mirroleap_update');
      if (request !== generation.current) return;
      if (!trustedReleaseUrl(result) ||
          !['update_available', 'up_to_date', 'ahead'].includes(result.status)) {
        setError(explainUpdateError('invalid_release', zh));
        return;
      }
      setInfo(result);
      if (result.status === 'update_available') {
        // Do not silently convert an extract-and-run portable ZIP into an
        // installed NSIS app. Only the main native window can answer this.
        try {
          const allowed = await invoke<boolean>('can_install_signed_update');
          if (request === generation.current) setInstallationAllowed(allowed === true);
        } catch {
          if (request === generation.current) setInstallationAllowed(false);
        }
      }
    } catch (reason) {
      if (request === generation.current) setError(explainUpdateError(reason, zh));
    } finally {
      lock.current = false;
      if (request === generation.current) setPending(false);
    }
  };

  const install = async () => {
    if (installLock.current || lock.current || !installationAllowed ||
        !info || info.status !== 'update_available' || !trustedReleaseUrl(info)) return;
    installLock.current = true;
    const request = generation.current;
    setInstalling(true);
    setError(null);
    setInstallPhase('downloading');
    setProgress({received:0,total:null});
    try {
      // The user may have switched to portable mode after checking. Also
      // reject racing/mismatched GitHub latest.json versions before install.
      if (await invoke<boolean>('can_install_signed_update') !== true) {
        throw new Error(zh ? '当前运行方式不支持直接安装，请使用官方 ZIP 更新。' :
          'This installation cannot use the updater. Use the official ZIP instead.');
      }
      const update = await checkSignedUpdate();
      if (!update || update.version.replace(/^v/, '') !== info.latestVersion) {
        throw new Error(zh ? '签名更新版本与已检查版本不一致，请重新检查更新。' :
          'The signed update version changed. Check for updates again.');
      }
      let received = 0;
      let total:number|null = null;
      await update.downloadAndInstall(event => {
        if (request !== generation.current) return;
        if (event.event === 'Started') {
          total = typeof event.data.contentLength === 'number' &&
            Number.isFinite(event.data.contentLength) && event.data.contentLength > 0 ?
            event.data.contentLength : null;
          received = 0;
          setProgress({received,total});
        } else if (event.event === 'Progress') {
          const length = event.data.chunkLength;
          if (Number.isFinite(length) && length > 0) received += length;
          setProgress({received,total});
        } else if (event.event === 'Finished') {
          setInstallPhase('installing');
        }
      });
      if (request === generation.current) setInstallPhase('installing');
    } catch (reason) {
      if (request === generation.current) {
        setError((zh ? '一键更新未完成：' : 'Signed update did not complete: ') +
          (reason instanceof Error ? reason.message : String(reason)));
        setInstallPhase(null);
      }
    } finally {
      installLock.current = false;
      if (request === generation.current) setInstalling(false);
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
        ? '发现新版本后，可验证签名并一键安装；免安装版继续使用 ZIP 更新。'
        : 'Check stable releases and install signed updates in one click. Portable ZIP updates stay manual.'}
    </p>
    <div className="flex flex-wrap items-center gap-3">
      <button type="button" onClick={() => void check()} disabled={pending || installing}
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
      {updateAvailable && installationAllowed && <div className="flex flex-col gap-2">
        <button type="button" disabled={installing || pending} onClick={() => void install()}
          aria-label={zh ? '验证签名并安装 MirroLeap 新版本' : 'Verify signature and install MirroLeap update'}
          className="ml-secondary-action inline-flex items-center self-start gap-2 px-4">
          {installing ? <LoaderCircle size={16} className="animate-spin"/> : <Download size={16}/>}
          {installing ? (zh ? '更新进行中…' : 'Installing update…') :
            (zh ? `一键更新到 v${info.latestVersion}` : `Update to v${info.latestVersion}`)}
        </button>
        {installing && <p className="ml-muted text-xs" role="progressbar"
          aria-valuemin={0} aria-valuemax={100}
          aria-valuenow={progress?.total ? Math.min(100,Math.round(progress.received * 100 / progress.total)) : undefined}>
          {installPhase === 'installing' ? (zh ? '下载完成，正在验证签名并准备安装…' : 'Download complete. Verifying signature and preparing installer…') :
            (zh ? `正在安全下载更新… ${progress?.total ? Math.round((progress.received / progress.total)*100)+'%' : ''}` :
              `Downloading signed update… ${progress?.total ? Math.round((progress.received / progress.total)*100)+'%' : ''}`)}
        </p>}
      </div>}
      {updateAvailable && !installationAllowed && <p className="ml-muted text-xs">
        {zh ? '当前版本为免安装模式或暂不支持程序内安装，请从下方选择官方 ZIP/安装包。' :
          'Portable or unsupported install: use the official ZIP/installer links below.'}
      </p>}
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
    <details className="ml-settings-help-details">
      <summary className="inline-flex items-center gap-2"><ShieldCheck size={14}/>{zh ? '下载与安全说明' : 'Download & safety'}</summary>
      <p>{zh ? '签名更新需要你手动确认并验证 Tauri 更新包签名，不会后台静默升级。Windows 发行商代码签名证书仍未配置，安装时可能弹出系统安全提示，请勿关闭 Windows 安全防护。' :
        'Signed updates require an explicit click and Tauri signature verification; no background installation. Windows publisher code signing is not configured, so system warnings may still appear. Never disable Windows security.'}</p>
    </details>
  </section>;
}
