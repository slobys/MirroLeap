import { fireEvent, render, screen, waitFor } from '@testing-library/react';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import AppUpdateChecker from '../AppUpdateChecker';
import type { AppUpdateInfo } from '../../utils/appUpdate';

const {invokeMock, signedUpdateMock} = vi.hoisted(() => ({
  invokeMock: vi.fn(),
  signedUpdateMock: vi.fn(),
}));
vi.mock('@tauri-apps/api/core', () => ({invoke: invokeMock}));
vi.mock('@tauri-apps/plugin-updater', () => ({check: signedUpdateMock}));

const update: AppUpdateInfo = {
  currentVersion: '4.8.1', latestVersion: '4.9.0', status: 'update_available',
  releaseUrl: 'https://github.com/slobys/MirroLeap/releases/tag/v4.9.0',
  publishedAt: '2026-10-16T01:00:00Z',
  changelog: 'Bug fixes and feature updates',
  downloads: [
    {
      kind: 'portable', name: 'MirroLeap_4.9.0_x64_Portable.zip',
      url: 'https://github.com/slobys/MirroLeap/releases/download/v4.9.0/MirroLeap_4.9.0_x64_Portable.zip',
      size: 5_000_000, sha256: 'a'.repeat(64),
    },
    {
      kind: 'exe', name: 'MirroLeap_4.9.0_x64-setup.exe',
      url: 'https://evil.test/hijack.exe', size: 5_000_000, sha256: null,
    },
  ],
};

describe('MirroLeap software update UI', () => {
  beforeEach(() => {
    invokeMock.mockReset();
    signedUpdateMock.mockReset();
  });

  it('never polls or installs automatically; user must request a check', () => {
    render(<AppUpdateChecker version="4.8.1"/>);
    expect(screen.getByText(/Installed version/)).toHaveTextContent('4.8.1');
    expect(screen.getByRole('link',{name:'Official releases'})).toHaveAttribute('href',
      'https://github.com/slobys/MirroLeap/releases');
    expect(invokeMock).not.toHaveBeenCalled();
  });

  it('shows a verified update with plain-text release notes and canonical official assets only', async () => {
    invokeMock.mockResolvedValue({...update, changelog: '<img src=x onerror=alert(1)>\nNew buttons'});
    const {container} = render(<AppUpdateChecker version="4.8.1"/>);
    fireEvent.click(screen.getByRole('button',{name:'Check for MirroLeap software updates'}));
    expect(invokeMock).toHaveBeenCalledExactlyOnceWith('check_mirroleap_update');
    expect(await screen.findByText(/Stable update v4.9.0 available/)).toBeInTheDocument();
    expect(screen.getByText(/New buttons/)).toBeInTheDocument();
    expect(container.querySelector('img')).toBeNull();
    expect(screen.getByRole('link',{name:'Portable ZIP'})).toHaveAttribute('href',
      update.downloads[0].url);
    expect(screen.queryByRole('link',{name:'EXE'})).not.toBeInTheDocument();
    expect(screen.getByRole('link',{name:'View this stable release'})).toHaveAttribute('href',update.releaseUrl);
    expect(container.querySelector('button[type=submit]')).not.toBeInTheDocument();
  });

  it('does not confuse an ahead/testing build with an upgrade or prompt installation', async () => {
    invokeMock.mockResolvedValue({...update,latestVersion:'4.7.2',
      releaseUrl:'https://github.com/slobys/MirroLeap/releases/tag/v4.7.2',
      status:'ahead'});
    render(<AppUpdateChecker version="4.8.1"/>);
    fireEvent.click(screen.getByRole('button',{name:'Check for MirroLeap software updates'}));
    expect(await screen.findByText(/Installed build is newer/)).toBeInTheDocument();
    expect(screen.queryByRole('link',{name:'Portable ZIP'})).not.toBeInTheDocument();
  });

  it('deduplicates checks and shows meaningful offline/rate-limit errors', async () => {
    let reject!: (reason:string) => void;
    invokeMock.mockReturnValueOnce(new Promise((_, rejectPromise) => {reject=rejectPromise;}));
    render(<AppUpdateChecker version="4.8.1"/>);
    const check=screen.getByRole('button',{name:'Check for MirroLeap software updates'});
    fireEvent.click(check);
    expect(check).toBeDisabled();
    fireEvent.click(check);
    expect(invokeMock).toHaveBeenCalledTimes(1);
    reject('rate_limited');
    expect(await screen.findByRole('alert')).toHaveTextContent('GitHub rate limit');
    await waitFor(() => expect(check).toBeEnabled());
  });

  it('rejects invalid GitHub Release metadata even if native IPC returns a syntactically valid object', async () => {
    invokeMock.mockResolvedValue({...update, releaseUrl:'https://evil.test/release'});
    render(<AppUpdateChecker version="4.8.1"/>);
    fireEvent.click(screen.getByRole('button',{name:'Check for MirroLeap software updates'}));
    expect(await screen.findByRole('alert')).toHaveTextContent('Could not verify');
    expect(screen.queryByRole('link',{name:'Portable ZIP'})).not.toBeInTheDocument();
  });

  it('installs only after an explicit click, matching GitHub stable version with signed manifest', async () => {
    const downloadAndInstall = vi.fn().mockResolvedValue(undefined);
    signedUpdateMock.mockResolvedValue({version:'4.9.0', downloadAndInstall});
    invokeMock.mockImplementation((cmd:string) => cmd === 'check_mirroleap_update' ?
      Promise.resolve(update) : Promise.resolve(true));
    render(<AppUpdateChecker version="4.8.1"/>);
    expect(signedUpdateMock).not.toHaveBeenCalled();
    fireEvent.click(screen.getByRole('button', {name:'Check for MirroLeap software updates'}));
    const button = await screen.findByRole('button',
      {name:'Verify signature and install MirroLeap update'});
    expect(signedUpdateMock).not.toHaveBeenCalled();
    fireEvent.click(button);
    await waitFor(() => expect(downloadAndInstall).toHaveBeenCalledTimes(1));
    expect(signedUpdateMock).toHaveBeenCalledTimes(1);
    expect(invokeMock).toHaveBeenCalledWith('can_install_signed_update');
  });

  it('never offers one-click upgrade from portable or unsupported installations', async () => {
    invokeMock.mockImplementation((cmd:string) => cmd === 'check_mirroleap_update' ?
      Promise.resolve(update) : Promise.resolve(false));
    render(<AppUpdateChecker version="4.8.1"/>);
    fireEvent.click(screen.getByRole('button', {name:'Check for MirroLeap software updates'}));
    expect(await screen.findByText(/Portable or unsupported install/)).toBeInTheDocument();
    expect(screen.queryByRole('button',
      {name:'Verify signature and install MirroLeap update'})).not.toBeInTheDocument();
    expect(signedUpdateMock).not.toHaveBeenCalled();
  });

  it('refuses an unsigned, missing or mismatched updater index', async () => {
    signedUpdateMock.mockResolvedValue({version:'5.9.9', downloadAndInstall:vi.fn()});
    invokeMock.mockImplementation((cmd:string) => cmd === 'check_mirroleap_update' ?
      Promise.resolve(update) : Promise.resolve(true));
    render(<AppUpdateChecker version="4.8.1"/>);
    fireEvent.click(screen.getByRole('button', {name:'Check for MirroLeap software updates'}));
    fireEvent.click(await screen.findByRole('button',
      {name:'Verify signature and install MirroLeap update'}));
    expect(await screen.findByRole('alert')).toHaveTextContent('The signed update version changed');
    expect((await signedUpdateMock.mock.results[0].value).downloadAndInstall).not.toHaveBeenCalled();
  });
});
