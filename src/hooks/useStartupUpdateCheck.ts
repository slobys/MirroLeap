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
  const [startupRelease, setStartupRelease] = useState<AppUpdateInfo | null>(null);
  const [startupInstallKind, setStartupInstallKind] = useState<InstallKind>('other');

  useEffect(() => {
    if (!enabled || requested.current || !('__TAURI_INTERNALS__' in window)) return;
    let active = true;
    const timer = setTimeout(() => {
      if (!active || requested.current) return;
      requested.current = true;
      void (async () => {
        try {
          const latest = await invoke<AppUpdateInfo>('check_mirroleap_update');
          if (!active || latest.status !== 'update_available' || !trustedReleaseUrl(latest)) return;
          const kind = await invoke<string>('get_app_install_kind').catch(() => 'other');
          if (!active) return;
          setStartupInstallKind(kind === 'installed' ? 'installed' :
            kind === 'portable' ? 'portable' : 'other');
          setStartupRelease(latest);
        } catch (error) {
          // No startup interruption, download, or installation on failures.
          console.info('[MirroLeap] startup update check unavailable:', error);
        }
      })();
    }, 1400);
    return () => { active = false; clearTimeout(timer); };
  }, [enabled]);

  return {
    startupRelease,
    startupInstallKind,
    dismissStartupUpdate: () => setStartupRelease(null),
  };
}
