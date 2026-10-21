/** MirroLeap desktop application update-check results. Distinct from updating
 * the Android scrcpy engine, which uses a completely separate workflow.
 */
export interface ReleaseDownload {
  kind: 'portable' | 'exe' | 'msi' | 'checksums';
  name: string;
  url: string;
  size: number;
  sha256: string | null;
}

export interface AppUpdateInfo {
  currentVersion: string;
  latestVersion: string;
  status: 'update_available' | 'up_to_date' | 'ahead';
  releaseUrl: string;
  publishedAt: string | null;
  changelog: string;
  downloads: ReleaseDownload[];
  /** The newest official signed release may be a public prerelease. */
  prerelease?: boolean;
  /** Signed channels have different Windows app identities and MUST NOT mix. */
  channel?: 'stable' | 'hq_preview';
}

/** Accept only exact canonical GitHub Release paths. This is defense-in-depth
 * in the UI; the Rust command already validates the tag and every asset URL.
 * Neither URL fragments, query parameters nor another host are allowed.
 */
export function trustedReleaseUrl(info: AppUpdateInfo): string | null {
  const version = info.latestVersion;
  if (!/^\d+(?:\.\d+){2}$/.test(version)) return null;
  if (info.channel === 'hq_preview') {
    // Only canonical signed HQ Preview prerelease tags in this repository.
    // Never treat arbitrary prereleases or stable vX tags as HQ updates.
    const prefix = `https://github.com/slobys/MirroLeap/releases/tag/hq-preview-v${version}-run`;
    if (!info.releaseUrl.startsWith(prefix)) return null;
    const run = info.releaseUrl.slice(prefix.length);
    return /^[1-9]\d*$/.test(run) ? info.releaseUrl : null;
  }
  if (info.channel && info.channel !== 'stable') return null;
  const expected = `https://github.com/slobys/MirroLeap/releases/tag/v${version}`;
  return info.releaseUrl === expected ? expected : null;
}

export function trustedReleaseDownload(info: AppUpdateInfo, asset: ReleaseDownload): string | null {
  if (!trustedReleaseUrl(info)) return null;
  const version = info.latestVersion;
  const isHq = info.channel === 'hq_preview';
  const allowed: Record<ReleaseDownload['kind'], string> = {
    portable: isHq ? '' : `MirroLeap_${version}_x64_Portable.zip`,
    exe: isHq ? `MirroLeap.HQ.Preview_${version}_x64-setup.exe` :
      `MirroLeap_${version}_x64-setup.exe`,
    msi: isHq ? `MirroLeap.HQ.Preview_${version}_x64_en-US.msi` :
      `MirroLeap_${version}_x64_en-US.msi`,
    checksums: 'SHA256SUMS.txt',
  };
  if (!allowed[asset.kind] || asset.name !== allowed[asset.kind] ||
      !Number.isFinite(asset.size) || asset.size <= 0 || asset.size > 500 * 1024 * 1024) {
    return null;
  }
  const releasePath = trustedReleaseUrl(info)?.split('/releases/tag/')[1];
  if (!releasePath) return null;
  const canonical = `https://github.com/slobys/MirroLeap/releases/download/${releasePath}/${asset.name}`;
  return asset.url === canonical ? canonical : null;
}

export type AppUpdateError =
  'rate_limited' | 'timeout' | 'network_error' | 'release_unavailable' |
  'metadata_too_large' | 'invalid_release' | 'invalid_local_version';

export function explainUpdateError(reason: unknown, zh: boolean): string {
  const code = typeof reason === 'string' ? reason : 'network_error';
  const chinese: Record<AppUpdateError, string> = {
    rate_limited: 'GitHub API 请求次数暂时受限，请稍后重试，或直接打开官方发布页。',
    timeout: '请求超时，请检查网络连接后重试。',
    network_error: '暂时无法连接 GitHub。请检查网络、代理或防火墙。',
    release_unavailable: 'GitHub 正式版本信息暂不可用，请稍后再试。',
    metadata_too_large: '版本信息超出安全读取限制，本次检查已停止。',
    invalid_release: '官方版本元数据无效，无法确认最新版本。',
    invalid_local_version: '本机版本号无法识别，请在官方发布页手动核对。',
  };
  const english: Record<AppUpdateError, string> = {
    rate_limited: 'GitHub rate limit reached. Retry later or use the official releases page.',
    timeout: 'The request timed out. Check your connection and retry.',
    network_error: 'Could not reach GitHub. Check your network or proxy.',
    release_unavailable: 'Official stable release information is temporarily unavailable.',
    metadata_too_large: 'The release metadata exceeded the safe size limit.',
    invalid_release: 'Could not verify the official stable release metadata.',
    invalid_local_version: 'The local app version is invalid. Check the official releases page.',
  };
  const valid = Object.prototype.hasOwnProperty.call(chinese, code) ? code as AppUpdateError : 'network_error';
  return (zh ? chinese : english)[valid];
}
