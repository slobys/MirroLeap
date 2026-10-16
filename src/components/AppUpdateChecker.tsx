import { useEffect, useRef, useState } from 'react';
import { CheckCircle2, Download, ExternalLink, LoaderCircle, RefreshCw, ShieldCheck } from 'lucide-react';
import { invoke } from '@tauri-apps/api/core';
import { useI18n } from '../i18n';
import { explainUpdateError, trustedReleaseDownload, trustedReleaseUrl,
  type AppUpdateInfo } from '../utils/appUpdate';

interface Props { version: string; running?: boolean; }

/**
 * Stable release metadata is checked only after a user click. Installation is
 * separately opt-in and uses Tauri's pinned-key signature verification; never
 * execute a GitHub-downloaded file directly. Portable ZIP remains manual.
 */
export default function AppUpdateChecker({version, running = false}: Props) {
  const {locale} = useI18n();
  const zh = locale.startsWith('zh');
  const [pending, setPending] = useState(false);
  const [info, setInfo] = useState<AppUpdateInfo | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [upgrading, setUpgrading] = useState(false);
  const [upgradeProgress, setUpgradeProgress] = useState<number | null>(null);
  const [upgradeMessage, setUpgradeMessage] = useState<string | null>(null);
  const generation = useRef(0);
  const lock = useRef(false);
  const upgradeLock = useRef(false);

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

  const installUpdate = async () => {
    if (lock.current || upgradeLock.current || !updateAvailable || !info || running) return;
    // The update must be both present in the official stable release metadata
    // and signed by the Tauri updater private key. A release ZIP or EXE cannot
    // bypass the updater signature simply because GitHub served it.
    upgradeLock.current = true;
    setUpgrading(true);
    setUpgradeMessage(null);
    setUpgradeProgress(null);
    try {
      const kind = await invoke<string>('get_app_install_kind');
      if (kind !== 'installed') {
        throw Error(zh
          ? '免安装版请使用下方 Portable ZIP 手动替换；一键安装只支持安装版。'
          : 'Portable ZIP requires a manual replacement. One-click install is for installed versions.');
      }
      const {check} = await import('@tauri-apps/plugin-updater');
      const signedUpdate = await check({timeout: 12000});
      if (!signedUpdate || signedUpdate.version !== info.latestVersion) {
        throw Error(zh
          ? '签名更新清单与官方正式版本不一致，已取消安装。'
          : 'Signed updater metadata does not match the official stable release.');
      }
      let received = 0;
      let length = 0;
      setUpgradeMessage(zh ? '正在下载并验证签名更新包…' : 'Downloading and verifying signed update…');
      await signedUpdate.downloadAndInstall(event => {
        if (event.event === 'Started') {
          length = Number(event.data.contentLength) || 0;
          received = 0;
          setUpgradeProgress(0);
        } else if (event.event === 'Progress') {
          received += event.data.chunkLength;
          if (length > 0) {
            setUpgradeProgress(Math.min(100, Math.floor(100 * received / length)));
          }
        } else if (event.event === 'Finished') {
          setUpgradeProgress(100);
          setUpgradeMessage(zh ? '签名验证已完成，正在安装…' : 'Signature verified. Installing…');
        }
      });
      setUpgradeMessage(zh ? '安装完成，正在重新启动…' : 'Installed. Restarting…');
      const {relaunch} = await import('@tauri-apps/plugin-process');
      await relaunch();
    } catch (reason) {
      setError((zh ? '在线更新未完成：' : 'One-click update not completed: ') +
        (reason instanceof Error ? reason.message : String(reason)));
      setUpgradeMessage(null);
    } finally {
      upgradeLock.current = false;
      setUpgrading(false);
    }
  };

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
        ? '检查 GitHub 正式版；安装版支持经签名验证的一键更新。'
        : 'Check GitHub stable releases. Installed editions support signed one-click updates.'}
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
      {updateAvailable && <button type="button" onClick={() => void installUpdate()}
        disabled={upgrading || pending || running}
        aria-label={zh ? '下载验证并安装 MirroLeap 更新' : 'Download, verify and install MirroLeap update'}
        className="ml-secondary-action inline-flex items-center gap-2 px-4">
        {upgrading ? <LoaderCircle size={16} className="animate-spin"/> : <Download size={16}/>}
        {upgrading ? (zh ? '正在安全更新…' : 'Updating securely…') :
          (zh ? '一键安装并重启' : 'Install update and restart')}
      </button>}
    </div>
    {running && updateAvailable && <p role="note" className="text-xs text-amber-600">
      {zh ? '先结束正在运行的投屏，再安装更新。' : 'Stop the active phone mirror before installing the update.'}
    </p>}
    {upgradeMessage && <p role="status" className="text-xs text-emerald-600">
      {upgradeMessage}{upgradeProgress !== null ? ` ${upgradeProgress}%` : ''}
    </p>}
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
    <details className="ml-settings-help-details">
      <summary className="inline-flex items-center gap-2"><ShieldCheck size={14}/>{zh ? '下载与安全说明' : 'Download & safety'}</summary>
      <p>{zh
        ? '一键更新使用 Tauri 签名公钥验证更新包，签名缺失或不匹配则拒绝安装。当前尚未配置受信任的 Windows 系统代码签名证书；SmartScreen 可能提示。免安装版请自行更新 ZIP，不要求关闭 Windows 安全防护。'
        : 'One-click updates require a Tauri-signed package; invalid or missing signatures fail closed. Windows Authenticode signing is not configured and SmartScreen may warn. Portable ZIP upgrades remain manual. Do not disable Windows security.'}</p>
    </details>
  </section>;
}
