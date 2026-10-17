import {StrictMode} from 'react';
import {act, renderHook} from '@testing-library/react';
import {afterEach, beforeEach, describe, expect, it, vi} from 'vitest';
import {useStartupUpdateCheck} from '../../hooks/useStartupUpdateCheck';
import type {AppUpdateInfo} from '../../utils/appUpdate';

const {invokeMock} = vi.hoisted(() => ({invokeMock: vi.fn()}));
vi.mock('@tauri-apps/api/core', () => ({invoke: invokeMock}));

const release: AppUpdateInfo = {
  currentVersion: '5.0.8', latestVersion: '5.0.9', status: 'update_available',
  releaseUrl: 'https://github.com/slobys/MirroLeap/releases/tag/v5.0.9',
  publishedAt: '2026-10-17T11:00:00Z', changelog: 'Verified update',
  downloads: [], prerelease: true,
};

describe('once-per-launch startup update check', () => {
  beforeEach(() => {
    vi.useFakeTimers();
    Object.defineProperty(window, '__TAURI_INTERNALS__',
      {value: {}, configurable: true});
    invokeMock.mockReset().mockImplementation(async (command: string) =>
      command === 'check_mirroleap_update' ? release : 'installed');
  });
  afterEach(() => {
    vi.useRealTimers();
    delete (window as Window & {__TAURI_INTERNALS__?: unknown}).__TAURI_INTERNALS__;
  });

  it('survives the StrictMode setup/cleanup replay and checks exactly once', async () => {
    const {result, rerender} = renderHook(() => useStartupUpdateCheck(true),
      {wrapper: StrictMode});
    expect(invokeMock).not.toHaveBeenCalled();
    await act(async () => { await vi.advanceTimersByTimeAsync(1400); });
    expect(result.current.startupRelease?.latestVersion).toBe('5.0.9');
    expect(result.current.showStartupPrompt).toBe(true);
    expect(result.current.startupInstallKind).toBe('installed');
    expect(invokeMock).toHaveBeenCalledTimes(2);
    expect(invokeMock).toHaveBeenCalledWith('check_mirroleap_update');
    rerender();
    await act(async () => { await vi.advanceTimersByTimeAsync(3000); });
    expect(invokeMock).toHaveBeenCalledTimes(2);
    act(() => result.current.dismissStartupPrompt());
    expect(result.current.showStartupPrompt).toBe(false);
    expect(result.current.startupRelease?.latestVersion).toBe('5.0.9');
    rerender();
    await act(async () => { await vi.advanceTimersByTimeAsync(2500); });
    expect(result.current.showStartupPrompt).toBe(false);
    expect(invokeMock).toHaveBeenCalledTimes(2);
  });

  it('does not consume the one check if disabled before a timer fires', async () => {
    const {result, rerender} = renderHook(({enabled}) => useStartupUpdateCheck(enabled),
      {initialProps: {enabled: true}});
    rerender({enabled: false});
    await act(async () => { await vi.advanceTimersByTimeAsync(2000); });
    expect(invokeMock).not.toHaveBeenCalled();
    rerender({enabled: true});
    await act(async () => { await vi.advanceTimersByTimeAsync(1400); });
    expect(result.current.startupRelease?.latestVersion).toBe('5.0.9');
    expect(result.current.showStartupPrompt).toBe(true);
    expect(invokeMock).toHaveBeenCalledTimes(2);
  });

  it('keeps the update status until a subsequent check confirms up to date', async () => {
    const {result}=renderHook(() => useStartupUpdateCheck(true));
    await act(async()=>{await vi.advanceTimersByTimeAsync(1400);});
    expect(result.current.startupRelease?.latestVersion).toBe('5.0.9');
    expect(result.current.showStartupPrompt).toBe(true);
    act(()=>result.current.rememberUpdate({...release,status:'up_to_date'},'other'));
    expect(result.current.startupRelease).toBeNull();
    expect(result.current.showStartupPrompt).toBe(false);
    act(()=>result.current.rememberUpdate(release,'installed'));
    expect(result.current.startupRelease?.latestVersion).toBe('5.0.9');
    expect(result.current.showStartupPrompt).toBe(false);
  });

  it('prompts again on a fresh app launch but never twice within one launch', async () => {
    const first = renderHook(() => useStartupUpdateCheck(true));
    await act(async () => { await vi.advanceTimersByTimeAsync(1400); });
    expect(first.result.current.showStartupPrompt).toBe(true);
    act(() => first.result.current.dismissStartupPrompt());
    expect(first.result.current.startupRelease?.latestVersion).toBe('5.0.9');
    first.unmount();
    const second = renderHook(() => useStartupUpdateCheck(true));
    await act(async () => { await vi.advanceTimersByTimeAsync(1400); });
    expect(second.result.current.showStartupPrompt).toBe(true);
    expect(invokeMock.mock.calls.filter(([action]) => action === 'check_mirroleap_update'))
      .toHaveLength(2);
    act(() => second.result.current.dismissStartupPrompt());
    await act(async () => { await vi.advanceTimersByTimeAsync(5000); });
    expect(second.result.current.showStartupPrompt).toBe(false);
    expect(invokeMock).toHaveBeenCalledTimes(4);
  });

  it('does not pop a dialog when GitHub says the app is already current', async () => {
    invokeMock.mockResolvedValueOnce({...release, status:'up_to_date'});
    const {result} = renderHook(() => useStartupUpdateCheck(true));
    await act(async () => { await vi.advanceTimersByTimeAsync(1400); });
    expect(result.current.startupRelease).toBeNull();
    expect(result.current.showStartupPrompt).toBe(false);
    expect(invokeMock).toHaveBeenCalledTimes(1);
  });

  it('never checks outside Tauri or after unmounting', async () => {
    delete (window as Window & {__TAURI_INTERNALS__?: unknown}).__TAURI_INTERNALS__;
    const first = renderHook(() => useStartupUpdateCheck(true));
    await act(async () => { await vi.advanceTimersByTimeAsync(2000); });
    expect(invokeMock).not.toHaveBeenCalled();
    first.unmount();
    Object.defineProperty(window, '__TAURI_INTERNALS__',
      {value: {}, configurable: true});
    const second = renderHook(() => useStartupUpdateCheck(true));
    second.unmount();
    await act(async () => { await vi.advanceTimersByTimeAsync(2000); });
    expect(invokeMock).not.toHaveBeenCalled();
  });

  it('never suggests a downgrade or untrusted release URL', async () => {
    invokeMock.mockResolvedValueOnce({...release, status: 'ahead'});
    const a = renderHook(() => useStartupUpdateCheck(true));
    await act(async () => { await vi.advanceTimersByTimeAsync(1400); });
    expect(a.result.current.startupRelease).toBeNull();
    expect(a.result.current.showStartupPrompt).toBe(false);
    a.unmount();
    invokeMock.mockReset().mockResolvedValueOnce({
      ...release, releaseUrl: 'https://evil.invalid/fake'});
    const b = renderHook(() => useStartupUpdateCheck(true));
    await act(async () => { await vi.advanceTimersByTimeAsync(1400); });
    expect(b.result.current.startupRelease).toBeNull();
    expect(b.result.current.showStartupPrompt).toBe(false);
    expect(invokeMock).toHaveBeenCalledTimes(1);
  });
});
