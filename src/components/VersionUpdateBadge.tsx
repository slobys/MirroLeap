import {RefreshCw} from 'lucide-react';

type Props = {
  version: string;
  locale: string;
  hasUpdate: boolean;
  checking?: boolean;
  onClick: () => void;
  showBrand?: boolean;
};

/** One shared entry point on both the home header and settings header. */
export default function VersionUpdateBadge({
  version, locale, hasUpdate, checking = false, onClick, showBrand = false,
}: Props) {
  const zh = locale.startsWith('zh');
  const title = hasUpdate
    ? (zh ? '发现新版，点击查看并决定是否更新' : 'Update available — click to review')
    : checking
      ? (zh ? '正在检查更新，点击可查看详情' : 'Checking updates — click for details')
      : (zh ? '点击检查 MirroLeap 更新' : 'Check MirroLeap updates');
  return <button type="button" className="ml-version-pill ml-version-action"
    onClick={onClick} title={title}
    aria-label={(showBrand ? 'MirroLeap ' : '') + 'v' + version + ' · ' + title}>
    {showBrand && <span>MirroLeap </span>}<span>v{version}</span>
    {hasUpdate
      ? <span className="ml-version-dot" title={zh ? '有新版本' : 'Update available'} aria-hidden="true"/>
      : checking
        ? <RefreshCw size={12} className="animate-spin" aria-hidden="true"/>
        : null}
  </button>;
}
