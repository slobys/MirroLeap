import { useEffect, useRef, useState, type MouseEvent } from 'react';
import { CheckCircle2, Download, ExternalLink, LoaderCircle, RefreshCw, ShieldCheck } from 'lucide-react';
import { invoke } from '@tauri-apps/api/core';
import {listen} from '@tauri-apps/api/event';
import { useI18n } from '../i18n';
import { explainUpdateError, trustedReleaseDownload, trustedReleaseUrl,
  type AppUpdateInfo } from '../utils/appUpdate';

interface Props {
  version: string;
  running?: boolean;
  initialInfo?: AppUpdateInfo | null;
  initialInstallKind?: 'installed' | 'portable' | 'other';
  onResult?: (info: AppUpdateInfo, kind: 'installed' | 'portable' | 'other') => void;
}

/**
 * Newest complete official release metadata is checked only after a user click. Installation is
 * separately opt-in and uses Tauri's pinned-key signature verification; never
 * execute a GitHub-downloaded file directly. Portable ZIP and MSI installs
 * stay on the manual path to avoid converting one installation into another.
 */
export default function AppUpdateChecker({
  version, running = false, initialInfo = null, initialInstallKind = 'other', onResult,
}: Props) {
  const {locale} = useI18n();
  const zh = locale.startsWith('zh');
  const [pending, setPending] = useState(false);
  const [info, setInfo] = useState<AppUpdateInfo | null>(initialInfo);
  const [error, setError] = useState<string | null>(null);
  const [upgrading, setUpgrading] = useState(false);
  const [upgradeProgress, setUpgradeProgress] = useState<number | null>(null);
  const [upgradeMessage, setUpgradeMessage] = useState<string | null>(null);
  const [installKind, setInstallKind] = useState<'unknown' | 'portable' | 'installed' | 'other'>(
    initialInfo ? initialInstallKind : 'unknown');
  const [downloadKind,setDownloadKind] = useState<string|null>(null);
  const [downloadProgress,setDownloadProgress] = useState<number|null>(null);
  const [downloadMessage,setDownloadMessage] = useState<string|null>(null);
  const downloadLock = useRef(false);
  const downloadKindRef = useRef<string|null>(null);
  const generation = useRef(0);
  const lock = useRef(false);
  const upgradeLock = useRef(false);

  useEffect(() => {
    // The browser-only visual fixture has no Tauri event IPC; event listeners
    // are strictly meaningful inside the real installed desktop WebView.
    if (!('__TAURI_INTERNALS__' in window)) {
      return () => { generation.current++; };
    }
    let active = true;
    const unlisten = listen<{percent?:number}>(
      'mirroleap-app-update-progress', ({payload}) => {
        if (!active || !upgradeLock.current) return;
        if (typeof payload?.percent === 'number' && Number.isFinite(payload.percent)) {
          setUpgradeProgress(Math.max(0, Math.min(100, Math.round(payload.percent))));
        }
      });
    const unlistenDownload = listen<{percent?:number,kind?:string}>(
      'mirroleap-manual-download-progress',({payload}) => {
        if (!active || !downloadLock.current || payload?.kind !== downloadKindRef.current) return;
        if (typeof payload?.percent === 'number' && Number.isFinite(payload.percent)) {
          setDownloadProgress(Math.max(0, Math.min(100, Math.round(payload.percent))));
        }
      });
    return () => {
      active = false;
      generation.current++;
      void unlisten.then(dispose => dispose());
      void unlistenDownload.then(dispose => dispose());
    };
  }, []);

  const check = async () => {
    if (lock.current) return;
    const request = ++generation.current;
    lock.current = true;
    setPending(true);
    setInfo(null);
    setInstallKind('unknown');
    setError(null);
    try {
      const result = await invoke<AppUpdateInfo>('check_mirroleap_update');
      if (request !== generation.current) return;
      if (!trustedReleaseUrl(result) ||
          !['update_available', 'up_to_date', 'ahead'].includes(result.status)) {
        setError(explainUpdateError('invalid_release', zh));
        return;
      }
      let kind: 'installed' | 'portable' | 'other' = 'other';
      if (result.status === 'update_available') {
        try {
          const detected = await invoke<string>('get_app_install_kind');
          if (request !== generation.current) return;
          kind = detected === 'installed' ? 'installed' :
            detected === 'portable' ? 'portable' : 'other';
        } catch {
          if (request !== generation.current) return;
        }
      }
      setInstallKind(kind);
      setInfo(result);
      onResult?.(result, kind);
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
  const openOfficialLink = (event:MouseEvent<HTMLAnchorElement>,kind:string) => {
    if (!('__TAURI_INTERNALS__' in window)) return;
    event.preventDefault();
    void invoke('open_official_release_link',{
      version:info?.latestVersion || version,kind,
    }).then(()=>{
      setDownloadMessage(zh?'已请求在系统默认浏览器中打开官方链接。':'The official link was handed to your default browser.');
    }).catch(reason=>setError((zh?'无法打开系统浏览器：':'Could not open the browser: ')+String(reason)));
  };
  const downloadOfficial = async(event:MouseEvent<HTMLAnchorElement>,kind:string) => {
    // Browser previews follow their GitHub anchors. The installed WebView2
    // saves directly to Downloads, avoiding blocked popup/download handling.
    if (!('__TAURI_INTERNALS__' in window)) return;
    event.preventDefault();
    if (!info || !trustedReleaseUrl(info) || downloadLock.current) return;
    downloadLock.current = true;
    downloadKindRef.current = kind;
    setDownloadKind(kind);
    setDownloadProgress(null);
    setDownloadMessage(null);
    setError(null);
    try {
      const path = await invoke<string>('download_official_release_asset',{
        version:info.latestVersion,kind,
      });
      setDownloadMessage(zh
        ? '文件已校验并保存到：' + path + '。不会自动运行安装程序。'
        : 'Verified file saved to: ' + path + '. The installer was not launched.');
    } catch(reason) {
      setError((zh?'下载失败，请尝试“查看该版本 GitHub 页面”：':'Download failed. Try the official release page: ')+String(reason));
    } finally {
      downloadLock.current = false;
      downloadKindRef.current = null;
      setDownloadKind(null);
    }
  };
  const updateAvailable = info?.status === 'update_available';

  const installUpdate = async () => {
    if (lock.current || upgradeLock.current || !updateAvailable || !info ||
        installKind !== 'installed' || running) return;
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
          ? '仅支持 NSIS EXE 安装版的一键升级；免安装 ZIP 和 MSI 安装版请下载对应文件手动升级。'
          : 'Signed one-click updates require an NSIS EXE installation. Portable ZIP and MSI versions must be upgraded manually.');
      }
      setUpgradeMessage(zh ? '正在下载并验证官方签名更新包…' : 'Downloading and verifying the official signed installer…');
      await invoke<void>('install_signed_update', {version:info.latestVersion});
      setUpgradeMessage(zh ? '更新安装器已启动，请按提示完成。' : 'Signed installer started. Follow its prompts.');
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
        ? '一键检查最新官方发布版本（包括测试版）。NSIS EXE 安装版可进行签名验证的一键升级。'
        : 'One click checks the newest signed official release, including previews. NSIS EXE installs support signature-verified in-app updates.'}
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
        onClick={event=>openOfficialLink(event,'list')}
        className="ml-footer-link inline-flex items-center gap-1">
        <ExternalLink size={13}/>
        {zh ? '官方发布页面' : 'Official releases'}
      </a>
      {updateAvailable && installKind === 'installed' && <button type="button" onClick={() => void installUpdate()}
        disabled={upgrading || pending || running}
        aria-label={zh ? '下载验证并安装 MirroLeap 更新' : 'Download, verify and install MirroLeap update'}
        className="ml-secondary-action inline-flex items-center gap-2 px-4">
        {upgrading ? <LoaderCircle size={16} className="animate-spin"/> : <Download size={16}/>}
        {upgrading ? (zh ? '正在安全更新…' : 'Updating securely…') :
          (zh ? '一键安装并重启' : 'Install update and restart')}
      </button>}
    </div>
    {updateAvailable && installKind !== 'installed' && <p role="note" className="ml-muted text-xs">
      {zh
        ? '当前未检测到可一键升级的 NSIS 安装。请使用下方官方 ZIP、EXE 或 MSI 文件手动升级。'
        : 'No eligible NSIS installation was detected. Use the official ZIP, EXE or MSI download links below to upgrade manually.'}
    </p>}
    {running && updateAvailable && installKind === 'installed' && <p role="note" className="text-xs text-amber-600">
      {zh ? '先结束正在运行的投屏，再安装更新。' : 'Stop the active phone mirror before installing the update.'}
    </p>}
    {upgradeMessage && <p role="status" className="text-xs text-emerald-600">
      {upgradeMessage}{upgradeProgress !== null ? ` ${upgradeProgress}%` : ''}
    </p>}
    {error && <p role="alert" className="text-xs text-amber-600 leading-relaxed">{error}</p>}
    {(downloadMessage || downloadKind) && <p role="status" className="text-xs text-emerald-600 break-all">
      {downloadKind ? (zh ? '正在校验并下载官方文件…' : 'Downloading and verifying official asset…') +
        (downloadProgress === null ? '' : ' ' + downloadProgress + '%') : downloadMessage}
    </p>}
    {info && <div role="status" aria-live="polite" className="flex flex-col gap-3 rounded-lg border border-[var(--border-base)] p-3">
      <div className="flex items-start gap-2">
        {updateAvailable ? <Download size={18} className="ml-primary shrink-0"/> :
          <CheckCircle2 size={18} className="text-emerald-600 shrink-0"/>}
        <div className="flex flex-col gap-1">
          <p className="font-semibold text-sm">
            {updateAvailable ? (zh ? '发现新版本' : 'New version') :
              info.status === 'ahead' ? (zh ? '当前版本高于已发布版本' : 'Installed build is newer than') :
              (zh ? '已是最新版本' : 'Up to date:')}
            {' '}v{info.latestVersion}{info.prerelease ? (zh ? ' · 测试版' : ' · Preview') : ''}
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
            onClick={event=>void downloadOfficial(event,asset.kind)}
            target="_blank" rel="noopener noreferrer"
            className="ml-footer-link inline-flex items-center gap-1 text-xs">
            {downloadKind === asset.kind ? <LoaderCircle size={13} className="animate-spin"/> : <Download size={13}/>}
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
        onClick={event=>openOfficialLink(event,'page')}
        className="ml-footer-link inline-flex items-center self-start gap-1">
        <ExternalLink size={13}/>{zh ? '查看该版本 GitHub 页面' : 'View this release'}
      </a>}
    </div>}
    <details className="ml-settings-help-details">
      <summary className="inline-flex items-center gap-2"><ShieldCheck size={14}/>{zh ? '下载与安全说明' : 'Download & safety'}</summary>
      <p>{zh
        ? '仅 NSIS EXE 安装版使用 Tauri 签名公钥验证更新包；签名无效则拒绝安装。免安装 ZIP 和 MSI 请手动更新。未配置受信任的 Windows 代码签名，SmartScreen 仍可能提示，请勿关闭系统防护。'
        : 'Only NSIS EXE installations use pinned-key Tauri-signed updates; invalid signatures fail closed. MSI and portable ZIP upgrades remain manual. No trusted Windows Authenticode certificate is configured; do not disable system protection.'}</p>
    </details>
  </section>;
}
