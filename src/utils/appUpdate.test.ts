import { describe, expect, it } from 'vitest';
import { explainUpdateError, trustedReleaseDownload, trustedReleaseUrl, type AppUpdateInfo } from './appUpdate';

const valid: AppUpdateInfo = {
  currentVersion: '4.8.1', latestVersion: '4.9.0', status: 'update_available',
  releaseUrl: 'https://github.com/slobys/MirroLeap/releases/tag/v4.9.0',
  publishedAt: '2026-10-16T01:00:00Z', changelog: 'Test release',
  downloads: [{
    kind: 'portable', name: 'MirroLeap_4.9.0_x64_Portable.zip',
    url: 'https://github.com/slobys/MirroLeap/releases/download/v4.9.0/MirroLeap_4.9.0_x64_Portable.zip',
    size: 5000000, sha256: 'a'.repeat(64),
  }],
};

describe('official MirroLeap release-link verification', () => {
  it('accepts only canonical tagged stable release pages and known download filenames', () => {
    expect(trustedReleaseUrl(valid)).toBe(valid.releaseUrl);
    expect(trustedReleaseDownload(valid, valid.downloads[0])).toBe(valid.downloads[0].url);
    const queryLink = {...valid.downloads[0], url: valid.downloads[0].url + '?download=1'};
    expect(trustedReleaseDownload(valid, queryLink)).toBeNull();
    expect(trustedReleaseDownload(valid, {...valid.downloads[0], size: 0})).toBeNull();
    expect(trustedReleaseDownload(valid, {...valid.downloads[0], name:'MirroLeap_4.9.0_x64-setup.exe'})).toBeNull();
  });

  it('rejects redirections, off-domain paths, prerelease tags or invalid metadata', () => {
    for (const releaseUrl of [
      'http://github.com/slobys/MirroLeap/releases/tag/v4.9.0',
      'https://github.com/other/MirroLeap/releases/tag/v4.9.0',
      valid.releaseUrl + '?tracking=yes',
      'https://github.com/slobys/MirroLeap/releases/tag/v4.9.0-beta',
      'https://github.com/slobys/MirroLeap/releases/tag/v4.9.0/../../evil',
    ]) {
      expect(trustedReleaseUrl({...valid, releaseUrl})).toBeNull();
    }
    expect(trustedReleaseUrl({...valid, latestVersion:'4.9.0-rc1'})).toBeNull();
    expect(trustedReleaseDownload({...valid, releaseUrl:'https://evil.test'},valid.downloads[0])).toBeNull();
  });

  it('isolates signed HQ Preview update links from stable releases', () => {
    const preview: AppUpdateInfo = {
      ...valid,channel:'hq_preview',prerelease:true,latestVersion:'5.2.6',
      releaseUrl:'https://github.com/slobys/MirroLeap/releases/tag/hq-preview-v5.2.6-run9',
      downloads:[{
        kind:'exe',name:'MirroLeap.HQ.Preview_5.2.6_x64-setup.exe',
        url:'https://github.com/slobys/MirroLeap/releases/download/hq-preview-v5.2.6-run9/MirroLeap.HQ.Preview_5.2.6_x64-setup.exe',
        size:21000000,sha256:null,
      }],
    };
    expect(trustedReleaseUrl(preview)).toBe(preview.releaseUrl);
    expect(trustedReleaseDownload(preview,preview.downloads[0])).toBe(preview.downloads[0].url);
    expect(trustedReleaseUrl({...preview,releaseUrl:valid.releaseUrl})).toBeNull();
    expect(trustedReleaseUrl({...preview,releaseUrl:preview.releaseUrl.replace('run9','run0')})).toBeNull();
    expect(trustedReleaseUrl({...preview,releaseUrl:preview.releaseUrl+'?tracking=1'})).toBeNull();
    expect(trustedReleaseDownload(preview,{...preview.downloads[0],name:'MirroLeap_5.2.6_x64-setup.exe'})).toBeNull();
    expect(trustedReleaseDownload(preview,{...preview.downloads[0],kind:'portable'})).toBeNull();
    expect(trustedReleaseDownload(preview,{...preview.downloads[0],url:preview.downloads[0].url.replace('hq-preview-v5.2.6-run9','v5.2.6')})).toBeNull();
    expect(trustedReleaseUrl({...preview,channel:'stable'})).toBeNull();
  });

  it('offers understandable error guidance in Chinese and English', () => {
    expect(explainUpdateError('rate_limited',true)).toMatch(/请求次数/);
    expect(explainUpdateError('timeout',false)).toMatch(/timed out/i);
    expect(explainUpdateError(new Error('SECRET'),true)).not.toMatch(/SECRET/);
  });
});
