import {useEffect, useRef, useState} from 'react';
import {invoke} from '@tauri-apps/api/core';
import {trustedReleaseUrl, type AppUpdateInfo} from '../utils/appUpdate';

type InstallKind = 'installed' | 'portable' | 'other';

/**
 * One read-only update check per desktop app launch. Do not mark the check as
 * started until its delay actually fires: React StrictMode or a transient
 * remount can clean up the first timer before it ever makes a network call.
 */
export function useStartupUpdateCheck(enabled: boolean) {
  const requested = useRef(false);
  const [checking, setChecking] = useState(false);
  const [startupRelease, setStartupRelease] = useState<AppUpdateInfo | null>(null);
  const [showStartupPrompt, setShowStartupPrompt] = useState(false);
  const [startupInstallKind, setStartupInstallKind] = useState<InstallKind>('other');

  useEffect(() => {
    if (!enabled || requested.current || !('__TAURI_INTERNALS__' in window)) return;
    let active = true;
    const timer = setTimeout(() => {
      if (!active || requested.current) return;
      requested.current = true;
      setChecking(true);
      void (async () => {
        try {
          const latest = await invoke<AppUpdateInfo>('check_mirroleap_update');
          if (!active || latest.status !== 'update_available' || !trustedReleaseUrl(latest)) return;
          const kind = await invoke<string>('get_app_install_kind').catch(() => 'other');
          if (!active) return;
          setStartupInstallKind(kind === 'installed' ? 'installed' :
            kind === 'portable' ? 'portable' : 'other');
          setStartupRelease(latest);
          // The version badge and once-per-launch opt-in dialog are distinct:
          // dismissing the dialog must never hide the green update badge.
          setShowStartupPrompt(true);
        } catch (error) {
          // No startup interruption, download, or installation on failures.
          console.info('[MirroLeap] startup update check unavailable:', error);
        } finally {
          if (active) setChecking(false);
        }
      })();
    }, 1400);
    return () => { active = false; clearTimeout(timer); };
  }, [enabled]);

  const rememberUpdate = (info: AppUpdateInfo, kind: InstallKind) => {
    setStartupRelease(info.status === 'update_available' && trustedReleaseUrl(info) ? info : null);
    setStartupInstallKind(kind);
    // Explicit checks from the version badge must never re-open the startup
    // dialog, including after the user already selected "Later".
    setShowStartupPrompt(false);
  };

  return {
    startupRelease,
    startupInstallKind,
    showStartupPrompt,
    checking,
    rememberUpdate,
    dismissStartupPrompt: () => setShowStartupPrompt(false),
  };
}
