import { useEffect, useState } from "react";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { open } from "@tauri-apps/plugin-dialog";
import Sidebar from "./components/Sidebar";
import ControlPanel from "./components/ControlPanel";
import LogPanel from "./components/LogPanel";
import ScreenshotTools from "./components/ScreenshotTools";
import AppUpdateChecker from "./components/AppUpdateChecker";
import StartupUpdateNotice from "./components/StartupUpdateNotice";
import VersionUpdateBadge from "./components/VersionUpdateBadge";
import DecoderPerformance from "./components/DecoderPerformance";
import Header from "./components/Header";
import SessionBehavior from "./components/SessionBehavior";
import Footer from "./components/Footer";
import CompactDashboard from "./components/CompactDashboard";
import ErrorBoundary from "./components/ErrorBoundary";
import OnboardingModal from "./components/OnboardingModal";
import ThemedModal from "./components/ThemedModal";
import { useScrcpy } from "./hooks/useScrcpy";
import { getVersion } from '@tauri-apps/api/app';
import { useI18n } from "./i18n";
import {useStartupUpdateCheck} from "./hooks/useStartupUpdateCheck";
import { ArrowLeft, Keyboard, MonitorPlay, Palette, Settings2, Smartphone, TerminalSquare, X } from 'lucide-react';
import './settings-center.css';

type SettingsTab = 'devices' | 'mirror' | 'behavior' | 'appearance' | 'tools';
const RESOLUTION_LABELS: Record<string, string> = {
  '3840': '4K', '2560': '2K', '1920': '1080p', '1600': '900p',
  '1280': '720p', '1024': '576p', '800': '480p',
};

function App() {
  const { t, locale } = useI18n();
  const {
    devices,
    deviceModels,
    pendingDevices,
    deviceFriendlyNames,
    logs,
    activeDevice,
    setActiveDevice,
    refreshDevices,
    refreshDevicesUntilSettled,
    runScrcpy,
    stopScrcpy,
    changeMirrorResolution,
    applyRecordingNow,
    isApplyingRecording,
    recordingStatuses,
    activeRecordingSessions,
    downloadScrcpy,
    checkScrcpy,
    scrcpyStatus,
    setLogs,
    isDownloading,
    downloadProgress,
    pairDevice,
    connectDevice,
    enableWirelessFromUsb,
    diagnoseConnection,
    listScrcpyOptions,
    runTerminalCommand,
    runningDevices,
    isSwitchingResolution,
    isRefreshing,
    sessionRunning,
    clearLogs,
    detectedCameras,
    renderDriverSupport,
    videoDecoderSupport,
    observedDecoders,
    mdnsDevices,
    config,
    setConfig,
    theme,
    setTheme,
    colorMode,
    setColorMode,
    pushFile,
    installApk,
    historyDevices,
    clearHistory,
    isOnboardingOpen,
    setIsOnboardingOpen,
    completeOnboarding
  } = useScrcpy();

  const [alertState, setAlertState] = useState<{
    isOpen: boolean;
    title: string;
    message: string;
    kind: 'warning' | 'error' | 'info' | 'success';
    actionLabel?: string;
    onAction?: () => void;
    showCancel?: boolean;
    cancelLabel?: string;
    onCancel?: () => void;
  }>({
    isOpen: false,
    title: '',
    message: '',
    kind: 'info'
  });

  const [appVersion, setAppVersion] = useState("5.0.14");
  const [appVersionReady,setAppVersionReady] = useState(false);
  const {startupRelease, startupInstallKind, showStartupPrompt,
    checking: checkingUpdate, rememberUpdate, dismissStartupPrompt} =
    useStartupUpdateCheck(appVersionReady);
  const [updateDialogOpen, setUpdateDialogOpen] = useState(false);
  const openUpdateCenter = () => {
    dismissStartupPrompt();
    setUpdateDialogOpen(true);
  };
  // Keep the daily workflow lightweight. Older saved scrcpy_* options are
  // still read by useScrcpy, and every advanced feature remains in Settings.
  const [view, setView] = useState<'home' | 'settings'>('home');
  const [settingsTab, setSettingsTab] = useState<SettingsTab>('mirror');
  const openSettings = (tab: SettingsTab = 'mirror') => {
    setSettingsTab(tab);
    setView('settings');
  };
  const [lastCheckedPath, setLastCheckedPath] = useState<string | undefined>(undefined);
  const [hasCheckedUpdate, setHasCheckedUpdate] = useState(false);

  const showAlert = (
    title: string,
    message: string,
    kind: 'warning' | 'error' | 'info' | 'success' = 'info',
    actionLabel = 'OK',
    onAction?: () => void,
    showCancel = false,
    cancelLabel = 'Cancel',
    onCancel?: () => void
  ) => {
    setAlertState({
      isOpen: true,
      title,
      message,
      kind,
      actionLabel,
      onAction,
      showCancel,
      cancelLabel,
      onCancel
    });
  };

  useEffect(() => {
    // Initial setup: fetch version and close splashscreen
    const initApp = async () => {
      try {
        const v = await getVersion();
        setAppVersion(v);
        setAppVersionReady(true);

        const { invoke } = await import('@tauri-apps/api/core');
        await invoke('close_splashscreen');
      } catch (e) {
        console.error("Initialization failed:", e);
      }
    };

    const timer = setTimeout(initApp, 500);
    return () => clearTimeout(timer);
  }, []);

  useEffect(() => {
    // Initial check (once on mount) - Silent to avoid log clatter. adb's own
    // reconnect of an already-paired device can lag a beat behind the app
    // opening, so keep checking briefly instead of a single refresh that
    // might land before adb has caught up.
    checkScrcpy(config.scrcpyPath);
    refreshDevicesUntilSettled(config.scrcpyPath);
  }, []);

  useEffect(() => {
    if (scrcpyStatus.found && (!hasCheckedUpdate || config.scrcpyPath !== lastCheckedPath) && !isDownloading) {
      setHasCheckedUpdate(true);
      setLastCheckedPath(config.scrcpyPath);
      
      const runCheck = async () => {
        try {
          const { invoke } = await import('@tauri-apps/api/core');
          const updateRes: any = await invoke('check_scrcpy_update', { customPath: config.scrcpyPath });
          if (updateRes && updateRes.update_available) {
            showAlert(
              t('alerts.updateAvailableTitle'),
              t('alerts.updateAvailableMessage', {
                local: updateRes.local_version || 'unknown',
                latest: updateRes.latest_version || 'unknown'
              }),
              'info',
              t('alerts.updateBtn'),
              async () => {
                // Preserve the current working core if download or validation fails.
                await downloadScrcpy();
              },
              true,
              t('alerts.cancelBtn')
            );
          }
        } catch (e) {
          console.error("Failed to check for scrcpy updates:", e);
        }
      };
      runCheck();
    } else if (!scrcpyStatus.found) {
      setHasCheckedUpdate(false);
    }
  }, [scrcpyStatus.found, config.scrcpyPath, isDownloading, hasCheckedUpdate, lastCheckedPath, t]);

  useEffect(() => {
    // Global Drag and Drop Listener (re-bind only if activeDevice changes)
    const unlisten = getCurrentWindow().listen<{ paths: string[] }>("tauri://drag-drop", (event) => {
      if (!activeDevice) {
        setLogs(prev => [...prev.slice(-100), t('logs.noDeviceForDragDrop')]);
        return;
      }

      const paths = event.payload.paths;
      if (paths && paths.length > 0) {
        paths.forEach(path => handleFileOperation(path));
      }
    });

    return () => {
      unlisten.then(f => f());
    };
  }, [activeDevice]);

  useEffect(() => {
    // Keep the Rust side's notion of "the selected device" in sync, so the
    // Ctrl+Alt+Shift+C global shortcut (a real OS-level hotkey registered in
    // shortcuts.rs, not a webview keydown listener) knows which mirror window
    // to recentre even when no app window has keyboard focus.
    const device = activeDevice && runningDevices.includes(activeDevice) ? activeDevice : null;
    (async () => {
      const { invoke } = await import('@tauri-apps/api/core');
      await invoke('set_active_device', { device });
    })();
  }, [activeDevice, runningDevices]);

  useEffect(() => {
    if (activeDevice) {
      setConfig(prev => ({ ...prev, device: activeDevice }));
    }
  }, [activeDevice]);

  const handleStart = async () => {
    if (!activeDevice) {
      showAlert(t('alerts.noDeviceSelectedTitle'), t('alerts.noDeviceSelectedMessage'), "warning");
      return;
    }
    const error = await runScrcpy(config);
    if (error) showAlert(locale.startsWith('zh') ? '投屏启动失败' : 'Could not start mirroring', error, 'error');
  };

  const handleStartMirror = async () => {
    if (!activeDevice) {
      showAlert(t('alerts.noDeviceSelectedTitle'), t('alerts.noDeviceSelectedMessage'), 'warning');
      return;
    }
    // The advanced settings may previously have selected camera/desktop mode.
    // The everyday home button ALWAYS starts screen mirroring explicitly.
    const mirrorConfig = { ...config, device: activeDevice, sessionMode: 'mirror' };
    setConfig(mirrorConfig);
    const error = await runScrcpy(mirrorConfig);
    if (error) showAlert(locale.startsWith('zh') ? '投屏启动失败' : 'Could not start mirroring', error, 'error');
  };

  const handleStop = async () => {
    if (!activeDevice) return;
    await stopScrcpy(activeDevice);
  };

  const handleRefresh = () => {
    refreshDevices();
  };

  const handleKillAdb = async () => {
    try {
      const { invoke } = await import('@tauri-apps/api/core');
      await invoke('kill_adb', { customPath: config.scrcpyPath });
      refreshDevices(config.scrcpyPath);
    } catch (e) {
      console.error(e);
    }
  };

  const handleFileOperation = async (path: string) => {
    if (!activeDevice) return;

    const isApk = path.toLowerCase().endsWith('.apk');
    if (isApk) {
      await installApk(activeDevice, path);
    } else {
      await pushFile(activeDevice, path);
    }
  };

  const handleFileBrowse = async () => {
    if (!activeDevice) return;
    try {
      const selected = await open({
        multiple: true,
        filters: [
          {
            name: 'All Files',
            extensions: ['*']
          },
          {
            name: 'Android App (APK)',
            extensions: ['apk']
          }
        ]
      });

      if (selected) {
        if (Array.isArray(selected)) {
          selected.forEach(path => handleFileOperation(path));
        } else {
          handleFileOperation(selected);
        }
      }
    } catch (e) {
      console.error(e);
    }
  };

  const handleSetPath = async () => {
    try {
      let startPath = config.scrcpyPath;
      if (!startPath) {
        const { invoke } = await import('@tauri-apps/api/core');
        startPath = await invoke<string>('get_scrcpy_bin_dir').catch(() => '');
      }
      const selected = await open({
        directory: true,
        multiple: false,
        defaultPath: startPath || undefined
      });
      if (selected && typeof selected === 'string') {
        setConfig(prev => ({ ...prev, scrcpyPath: selected }));
        setLogs(prev => [...prev.slice(-100), t('logs.customScrcpyPathSet', { path: selected })]);
        // Trigger a check with the new path
        setTimeout(() => checkScrcpy(selected), 100);
      }
    } catch (e) {
      console.error(e);
    }
  };

  const handleResetPath = async () => {
    setConfig(prev => ({ ...prev, scrcpyPath: undefined }));
    setLogs(prev => [...prev.slice(-100), t('logs.customScrcpyPathCleared')]);
    // Trigger a check with no custom path
    setTimeout(() => checkScrcpy(undefined), 100);
  };

  return (
    <ErrorBoundary>
      <div className="min-h-screen font-sans selection:bg-primary selection:text-on-primary overflow-hidden flex flex-col transition-opacity duration-1000 ease-in-out" style={{ backgroundColor: 'var(--bg-base)', color: 'var(--text-base)', opacity: 0, animation: 'fadeIn 0.8s ease-out forwards' }}>
        <style>{`
          @keyframes fadeIn {
            from { opacity: 0; transform: translateY(5px); }
            to { opacity: 1; transform: translateY(0); }
          }
        `}</style>
        <div className="fixed inset-0 bg-[radial-gradient(ellipse_at_75%_5%,var(--primary),transparent_64%)] opacity-[0.075] pointer-events-none z-0" />

        <div className="relative z-10 flex flex-col h-screen min-h-0">
          {view === 'home' ? (
            <CompactDashboard
              locale={locale}
              version={appVersion}
              hasUpdate={Boolean(startupRelease)}
              checkingUpdate={checkingUpdate}
              onOpenUpdate={openUpdateCenter}
              devices={devices}
              pendingDevices={pendingDevices}
              deviceModels={deviceModels}
              deviceFriendlyNames={deviceFriendlyNames}
              runningDevices={runningDevices}
              activeDevice={activeDevice}
              onSelectDevice={setActiveDevice}
              onRefresh={handleRefresh}
              isRefreshing={isRefreshing}
              config={config}
              onConfigChange={setConfig}
              onResolutionChange={(size) => {
                if (config.sessionMode === 'mirror') {
                  void changeMirrorResolution(activeDevice || config.device, size);
                } else {
                  setConfig(prev => ({ ...prev, res: size }));
                }
              }}
              isSwitchingResolution={isSwitchingResolution}
              onStart={handleStartMirror}
              onStop={handleStop}
              onOpenSettings={openSettings}
              coreReady={scrcpyStatus.found}
              onDownloadCore={downloadScrcpy}
              isDownloading={isDownloading}
              downloadProgress={downloadProgress}
            />
          ) : (
            <section className="ml-settings flex flex-col flex-1 min-h-0" aria-label={locale.startsWith('zh') ? '设置' : 'Settings'}>
              <div className="ml-settings-top shrink-0 px-5 sm:px-8 pt-6 pb-4">
                <div className="flex items-center gap-3">
                  <button type="button" className="ml-icon-button" aria-label={locale.startsWith('zh') ? '返回首页' : 'Back to home'}
                    onClick={() => setView('home')}><ArrowLeft size={19}/></button>
                  <div className="min-w-0 flex-1">
                    <h1 className="font-bold text-xl flex items-center gap-2"><Settings2 size={20} className="ml-primary"/> {locale.startsWith('zh') ? '设置与高级功能' : 'Settings & advanced features'}</h1>
                    <p className="ml-muted text-xs mt-0.5">{locale.startsWith('zh') ? '常用功能在首页，全部专业选项在这里。' : 'Every advanced option remains available here.'}</p>
                  </div>
                  <VersionUpdateBadge version={appVersion} locale={locale} showBrand
                    hasUpdate={Boolean(startupRelease)} checking={checkingUpdate}
                    onClick={openUpdateCenter}/>
                </div>
                <nav className="ml-settings-nav flex gap-2 overflow-x-auto custom-scrollbar mt-4 pb-1" aria-label={locale.startsWith('zh') ? '设置分类' : 'Settings categories'}>
                  {([
                    { id: 'mirror' as const, Icon: MonitorPlay, zh: '投屏参数', en: 'Capture' },
                    { id: 'devices' as const, Icon: Smartphone, zh: '连接与配对', en: 'Connections' },
                    { id: 'behavior' as const, Icon: Keyboard, zh: '行为设置', en: 'Behavior settings' },
                    { id: 'appearance' as const, Icon: Palette, zh: '外观与引擎', en: 'Appearance' },
                    { id: 'tools' as const, Icon: TerminalSquare, zh: '工具与日志', en: 'Tools & logs' },
                  ]).map(({id, Icon, zh, en}) => (
                    <button key={id} type="button" aria-current={settingsTab === id ? 'page' : undefined}
                      onClick={() => setSettingsTab(id)}
                      className={'ml-settings-tab ' + (settingsTab === id ? 'ml-tab-active' : '')}>
                      <Icon size={16}/> {locale.startsWith('zh') ? zh : en}
                    </button>
                  ))}
                </nav>
              </div>
              <div className="ml-settings-body flex-1 min-h-0 overflow-y-auto custom-scrollbar px-5 sm:px-8 pb-12 pt-6" key={settingsTab}>
                <div className="ml-settings-frame mx-auto">
                  {settingsTab === 'mirror' && (
                    <div className="ml-settings-columns ml-mirror-layout">
                      <ControlPanel
                        config={config} setConfig={setConfig} onStart={handleStart}
                        onStop={handleStop} isRunning={sessionRunning}
                        onResolutionChange={(size) => void changeMirrorResolution(activeDevice || config.device, size)}
                        isSwitchingResolution={isSwitchingResolution} detectedCameras={detectedCameras}
                        renderDriverSupport={renderDriverSupport}
                        onListOptions={(arg) => { if (activeDevice) listScrcpyOptions(activeDevice, arg); }}
                      />
                      <aside className="ml-settings-stack" aria-label={locale.startsWith('zh') ? '解码与当前配置' : 'Decoder and current settings'}>
                        <DecoderPerformance config={config} setConfig={setConfig}
                          support={videoDecoderSupport}
                          observedMode={observedDecoders[activeDevice]}
                          running={sessionRunning}/>
                        <section className="ml-card ml-settings-summary">
                          <h2>{locale.startsWith('zh') ? '当前参数概览' : 'Configuration at a glance'}</h2>
                          <p className="ml-muted">{locale.startsWith('zh') ? '以下是已选择的参数，部分修改在下次投屏时生效。' : 'Selected values; some changes apply on the next session.'}</p>
                          <dl>
                            <div><dt>{t('controlPanel.resolution')}</dt><dd>{config.res === '0' || !config.res ? (locale.startsWith('zh') ? '原始' : 'Original') : RESOLUTION_LABELS[String(config.res)] || String(config.res)}</dd></div>
                            <div><dt>{t('controlPanel.codec')}</dt><dd>{String(config.codec || 'h264').toUpperCase()}</dd></div>
                            <div><dt>{t('controlPanel.fps')}</dt><dd>{config.fps || (locale.startsWith('zh') ? '自动' : 'Auto')}</dd></div>
                            <div><dt>{t('controlPanel.bitrate')}</dt><dd>{config.bitrate || 8} Mbps</dd></div>
                          </dl>
                        </section>
                      </aside>
                    </div>
                  )}
                  {settingsTab === 'devices' && (
                    <Sidebar
                      devices={devices} pendingDevices={pendingDevices} deviceModels={deviceModels}
                      deviceFriendlyNames={deviceFriendlyNames} runningDevices={runningDevices}
                      onRefresh={handleRefresh} onKillAdb={handleKillAdb}
                      selectedDevice={activeDevice} onSelectDevice={setActiveDevice}
                      onPair={pairDevice} onConnect={connectDevice} onUsbToWireless={enableWirelessFromUsb}
                      onDiagnose={diagnoseConnection} isRefreshing={isRefreshing}
                      onFilePush={handleFileBrowse} historyDevices={historyDevices}
                      clearHistory={clearHistory} mdnsDevices={mdnsDevices}
                    />
                  )}
                  {settingsTab === 'behavior' && (
                    <div className="ml-behavior-layout">
                      <SessionBehavior config={config} setConfig={setConfig}
                        isRunning={sessionRunning}
                        activeRecording={activeRecordingSessions[activeDevice]}
                        recordingStatus={recordingStatuses[activeDevice]}
                        isApplyingRecording={isApplyingRecording}
                        onApplyRecordingNow={() => applyRecordingNow(activeDevice || config.device, config)}/>
                    </div>
                  )}
                  {settingsTab === 'appearance' && (
                    <div className="ml-settings-columns ml-appearance-layout">
                      <Header
                        onThemeChange={setTheme} currentTheme={theme}
                        colorMode={colorMode} onColorModeChange={setColorMode}
                        binaryStatus={scrcpyStatus} onDownload={downloadScrcpy}
                        onSetPath={handleSetPath} onResetPath={handleResetPath}
                        isDownloading={isDownloading} downloadProgress={downloadProgress}
                        version={appVersion}
                      />
                      <div className="ml-settings-stack">
                        <Footer version={appVersion}/>
                      </div>
                    </div>
                  )}
                  {settingsTab === 'tools' && (
                    <div className="ml-settings-columns ml-tools-layout">
                      <div className="ml-settings-stack">
                        <ScreenshotTools device={activeDevice} customPath={config.scrcpyPath}/>
                      </div>
                      <aside className="ml-settings-stack">
                        <LogPanel logs={logs} onClear={clearLogs}
                          onAddLog={(msg) => setLogs((prev: string[]) => [...prev.slice(-100), msg])}
                          onRunCommand={runTerminalCommand}/>
                      </aside>
                    </div>
                  )}
                </div>
              </div>
            </section>
          )}
        </div>

        <OnboardingModal
          isOpen={isOnboardingOpen}
          onClose={() => setIsOnboardingOpen(false)}
          binaryStatus={scrcpyStatus}
          onDownload={downloadScrcpy}
          isDownloading={isDownloading}
          downloadProgress={downloadProgress}
          onComplete={completeOnboarding}
        />

        {showStartupPrompt && startupRelease && !updateDialogOpen && !isOnboardingOpen &&
          <StartupUpdateNotice release={startupRelease} installKind={startupInstallKind}
            running={sessionRunning} onDismiss={dismissStartupPrompt}/>}

        {updateDialogOpen && <div role="presentation"
          className="fixed inset-0 z-[450] flex items-center justify-center p-4"
          style={{background:'rgba(6,12,24,.62)',backdropFilter:'blur(5px)'}}>
          <section role="dialog" aria-modal="true" aria-labelledby="update-center-heading"
            className="ml-card w-full max-w-[650px] max-h-[88vh] overflow-y-auto p-5 shadow-2xl border border-[var(--border-base)]">
            <div className="flex items-center justify-between gap-4 mb-3">
              <h2 id="update-center-heading" className="font-bold text-lg">
                {locale.startsWith('zh') ? 'MirroLeap 版本与更新' : 'MirroLeap versions & updates'}
              </h2>
              <button type="button" className="ml-icon-button"
                aria-label={locale.startsWith('zh') ? '关闭更新窗口' : 'Close updates'}
                onClick={() => setUpdateDialogOpen(false)}><X size={18}/></button>
            </div>
            <AppUpdateChecker version={appVersion} running={sessionRunning}
              initialInfo={startupRelease} initialInstallKind={startupInstallKind}
              onResult={rememberUpdate}/>
          </section>
        </div>}

        <ThemedModal
          isOpen={alertState.isOpen}
          onClose={() => setAlertState(prev => ({ ...prev, isOpen: false }))}
          title={alertState.title}
          message={alertState.message}
          kind={alertState.kind}
          actionLabel={alertState.actionLabel}
          onAction={alertState.onAction}
          showCancel={alertState.showCancel}
          cancelLabel={alertState.cancelLabel}
          onCancel={alertState.onCancel}
        />
      </div>
    </ErrorBoundary>
  );
}

export default App;
