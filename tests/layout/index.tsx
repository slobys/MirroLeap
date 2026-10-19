/**
 * DEV-ONLY layout fixture: renders the production CompactDashboard and CSS,
 * without Android/ADB so it is safe to simulate a running session. This page
 * is not included in the production Tauri Vite build.
 */
import React from 'react';
import ReactDOM from 'react-dom/client';
import '../../src/index.css';
import CompactDashboard from '../../src/components/CompactDashboard';
import type { ScrcpyConfig } from '../../src/hooks/useScrcpy';

const params = new URLSearchParams(location.search);
const mode = params.get('mode') || 'ready';
const selected = '192.168.2.226:5555';
const config: ScrcpyConfig = {
  device: selected,
  sessionMode: 'mirror',
  res: '0',
  bitrate: 8,
  audioEnabled: true,
  deviceFrame: true,
};

document.documentElement.setAttribute('data-mode', 'light');
document.documentElement.setAttribute('data-theme', 'ultraviolet');
document.documentElement.style.height = '100%';
document.body.style.height = '100vh';
document.getElementById('root')!.style.height = '100vh';

ReactDOM.createRoot(document.getElementById('root')!).render(
  <CompactDashboard
    locale="zh-CN"
    version="4.6.5"
    devices={[selected, 'usb-device']}
    pendingDevices={[]}
    deviceModels={{}}
    deviceFriendlyNames={{[selected]: 'Xiaomi 14 Ultra'}}
    runningDevices={mode === 'running' ? [selected] : []}
    activeDevice={selected}
    onSelectDevice={() => {}}
    onRefresh={() => {}}
    isRefreshing={false}
    config={config}
    onConfigChange={() => {}}
    onResolutionChange={() => {}}
    isSwitchingResolution={mode === 'switching'}
    onStart={() => {}}
    onStop={() => {}}
    onOpenSettings={() => {}}
    coreReady={mode !== 'no-core'}
    onDownloadCore={() => {}}
    isDownloading={false}
    downloadProgress={0}
  />,
);
