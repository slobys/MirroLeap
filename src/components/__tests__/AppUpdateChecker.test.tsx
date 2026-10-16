import { fireEvent, render, screen, waitFor } from '@testing-library/react';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import AppUpdateChecker from '../AppUpdateChecker';
import type { AppUpdateInfo } from '../../utils/appUpdate';

const {invokeMock} = vi.hoisted(() => ({invokeMock: vi.fn()}));
vi.mock('@tauri-apps/api/core', () => ({invoke: invokeMock}));
const { updaterCheck, updaterInstall, updaterRelaunch } = vi.hoisted(() => ({
  updaterCheck: vi.fn(), updaterInstall: vi.fn(), updaterRelaunch: vi.fn(),
}));
vi.mock('@tauri-apps/plugin-updater', () => ({check: updaterCheck}));
vi.mock('@tauri-apps/plugin-process', () => ({relaunch: updaterRelaunch}));

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
    updaterCheck.mockReset();
    updaterInstall.mockReset();
    updaterRelaunch.mockReset();
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
    expect(invokeMock).toHaveBeenCalledWith('check_mirroleap_update');
    expect(await screen.findByText(/Stable update v4.9.0 available/)).toBeInTheDocument();
    expect(screen.getByText(/New buttons/)).toBeInTheDocument();
    expect(container.querySelector('img')).toBeNull();
    expect(screen.getByRole('link',{name:'Portable ZIP'})).toHaveAttribute('href',
      update.downloads[0].url);
    expect(screen.queryByRole('link',{name:'EXE'})).not.toBeInTheDocument();
    expect(screen.getByRole('link',{name:'View this stable release'})).toHaveAttribute('href',update.releaseUrl);
    expect(container.querySelector('button[type=submit]')).not.toBeInTheDocument();
  });

  it('denies one-click install to portable ZIP and never invokes updater', async () => {
    invokeMock.mockImplementation(async (action:string) =>
      action === 'get_app_install_kind' ? 'portable' : update);
    render(<AppUpdateChecker version="4.8.1"/>);
    fireEvent.click(screen.getByRole('button',{name:'Check for MirroLeap software updates'}));
    await screen.findByText(/Stable update v4.9.0 available/);
    expect(screen.queryByRole('button',{name:'Download, verify and install MirroLeap update'}))
      .not.toBeInTheDocument();
    expect(screen.getByText(/No eligible NSIS installation was detected/)).toBeInTheDocument();
    expect(updaterCheck).not.toHaveBeenCalled();
  });

  it('also hides installer action for MSI or unknown Windows installations', async () => {
    invokeMock.mockImplementation(async (action:string) =>
      action === 'get_app_install_kind' ? 'other' : update);
    render(<AppUpdateChecker version="4.8.1"/>);
    fireEvent.click(screen.getByRole('button',{name:'Check for MirroLeap software updates'}));
    await screen.findByText(/Stable update v4.9.0 available/);
    expect(screen.queryByRole('button',{name:'Download, verify and install MirroLeap update'}))
      .not.toBeInTheDocument();
    expect(screen.getByRole('link',{name:'Portable ZIP'})).toHaveAttribute('href', update.downloads[0].url);
    expect(updaterCheck).not.toHaveBeenCalled();
  });

  it('rejects inconsistent signed manifest versions without installing', async () => {
    invokeMock.mockImplementation(async (action:string) =>
      action === 'get_app_install_kind' ? 'installed' : update);
    updaterCheck.mockResolvedValueOnce({version:'4.9.1',downloadAndInstall:updaterInstall});
    render(<AppUpdateChecker version="4.8.1"/>);
    fireEvent.click(screen.getByRole('button',{name:'Check for MirroLeap software updates'}));
    await screen.findByText(/Stable update v4.9.0 available/);
    fireEvent.click(screen.getByRole('button',{name:'Download, verify and install MirroLeap update'}));
    expect(await screen.findByRole('alert')).toHaveTextContent('does not match the official stable release');
    expect(updaterInstall).not.toHaveBeenCalled();
    expect(updaterRelaunch).not.toHaveBeenCalled();
  });

  it('starts the signed installer only after an explicit user click', async () => {
    invokeMock.mockImplementation(async (action:string) =>
      action === 'get_app_install_kind' ? 'installed' : update);
    updaterCheck.mockResolvedValueOnce({version:'4.9.0',downloadAndInstall:updaterInstall});
    updaterInstall.mockResolvedValue(undefined);
    updaterRelaunch.mockResolvedValue(undefined);
    render(<AppUpdateChecker version="4.8.1"/>);
    expect(updaterCheck).not.toHaveBeenCalled();
    fireEvent.click(screen.getByRole('button',{name:'Check for MirroLeap software updates'}));
    await screen.findByText(/Stable update v4.9.0 available/);
    fireEvent.click(screen.getByRole('button',{name:'Download, verify and install MirroLeap update'}));
    await waitFor(() => expect(updaterRelaunch).toHaveBeenCalledOnce());
    expect(updaterCheck).toHaveBeenCalledOnce();
    expect(updaterInstall).toHaveBeenCalledOnce();
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

  it('checks official preview releases only when requested, without auto-installing', async () => {
    invokeMock.mockImplementation(async (command:string) => {
      if (command === 'check_mirroleap_preview_update') {
        return {...update, latestVersion:'5.0.5',
          releaseUrl:'https://github.com/slobys/MirroLeap/releases/tag/v5.0.5',
          downloads:[{...update.downloads[0],
            name:'MirroLeap_5.0.5_x64_Portable.zip',
            url:'https://github.com/slobys/MirroLeap/releases/download/v5.0.5/MirroLeap_5.0.5_x64_Portable.zip'}]};
      }
      throw Error('Unexpected API');
    });
    render(<AppUpdateChecker version="5.0.4"/>);
    expect(invokeMock).not.toHaveBeenCalled();
    fireEvent.click(screen.getByRole('button',{name:'Check MirroLeap preview updates'}));
    expect(await screen.findByText('Preview update v5.0.5 available')).toBeInTheDocument();
    expect(invokeMock).toHaveBeenCalledExactlyOnceWith('check_mirroleap_preview_update');
    expect(screen.getByRole('link',{name:'Portable ZIP'}))
      .toHaveAttribute('href','https://github.com/slobys/MirroLeap/releases/download/v5.0.5/MirroLeap_5.0.5_x64_Portable.zip');
    expect(screen.queryByRole('button',{name:'Download, verify and install MirroLeap update'}))
      .not.toBeInTheDocument();
    expect(updaterCheck).not.toHaveBeenCalled();
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
});
