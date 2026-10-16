/**
 * Dev-only, real React/CSS settings fixture for Chromium/Edge geometry tests.
 * The capture and behavior sections below mount actual production components.
 * No ADB, filesystem or Tauri calls are made by this fixture.
 */
import { useState } from 'react';
import ReactDOM from 'react-dom/client';
import { ArrowLeft, Keyboard, MonitorPlay, Palette, Settings2, Smartphone, TerminalSquare } from 'lucide-react';
import '../../src/index.css';
import { I18nProvider } from '../../src/i18n';
import ControlPanel from '../../src/components/ControlPanel';
import SessionBehavior from '../../src/components/SessionBehavior';
import ShortcutsPanel from '../../src/components/ShortcutsPanel';
import type { ScrcpyConfig } from '../../src/hooks/useScrcpy';

const tabs = [
  { id: 'mirror', text: '投屏参数', Icon: MonitorPlay },
  { id: 'devices', text: '连接与配对', Icon: Smartphone },
  { id: 'behavior', text: '行为与快捷键', Icon: Keyboard },
  { id: 'appearance', text: '外观与引擎', Icon: Palette },
  { id: 'tools', text: '工具与日志', Icon: TerminalSquare },
] as const;
type Tab = typeof tabs[number]['id'];

function SettingsFixture() {
  const [tab, setTab] = useState<Tab>('mirror');
  const [config, setConfig] = useState<ScrcpyConfig>({
    device: 'regression-device',
    sessionMode: 'mirror',
    res: '1920',
    codec: 'h264',
    fps: 60,
    bitrate: 8,
    hidKeyboard: true,
  });

  return (
    <section className="ml-settings flex flex-col flex-1 min-h-0" aria-label="设置">
      <div className="ml-settings-top shrink-0 px-5 sm:px-8 pt-6 pb-4">
        <div className="flex items-center gap-3">
          <button type="button" className="ml-icon-button" aria-label="返回首页"><ArrowLeft size={19}/></button>
          <div className="min-w-0 flex-1">
            <h1 className="font-bold text-xl flex items-center gap-2"><Settings2 size={20} className="ml-primary"/> 设置与高级功能</h1>
            <p className="ml-muted text-xs mt-0.5">常用功能在首页，全部专业选项在这里。</p>
          </div>
          <span className="ml-version-pill">MirroLeap v5.0.1</span>
        </div>
        <nav className="ml-settings-nav flex gap-2 overflow-x-auto custom-scrollbar mt-4 pb-1" aria-label="设置分类">
          {tabs.map(({id, text, Icon}) => (
            <button key={id} type="button" aria-current={tab === id ? 'page' : undefined}
              onClick={() => setTab(id)} className={'ml-settings-tab ' + (tab === id ? 'ml-tab-active' : '')}>
              <Icon size={16}/>{text}
            </button>
          ))}
        </nav>
      </div>
      <div className="ml-settings-body flex-1 min-h-0 overflow-y-auto custom-scrollbar px-5 sm:px-8 pb-12 pt-6" key={tab}>
        <div className="max-w-[1080px] mx-auto">
          {tab === 'mirror' && <div className="flex flex-col gap-6">
            <ControlPanel config={config} setConfig={setConfig} isRunning={false}
              onStart={() => {}} onStop={() => {}} onListOptions={() => {}}
              renderDriverSupport={{hostOs: 'windows',supportsRenderDriver:true,
                supportedDrivers:[{id:'direct3d',label:'D3D11 (Direct3D)'},{id:'opengl',label:'OpenGL'}]}}/>
          </div>}
          {tab === 'behavior' && <div className="grid grid-cols-1 lg:grid-cols-2 gap-6">
            <SessionBehavior config={config} setConfig={setConfig}/>
            <ShortcutsPanel/>
          </div>}
          {tab !== 'mirror' && tab !== 'behavior' && <p className="ml-muted">其他设置共用相同的滚动容器。</p>}
        </div>
      </div>
    </section>
  );
}

document.documentElement.setAttribute('data-mode','light');
document.documentElement.setAttribute('data-theme','ultraviolet');
document.documentElement.style.height = '100%';
document.body.style.height = '100vh';
const root = document.getElementById('root')!;
root.style.height = '100vh';
root.style.display = 'flex';
root.style.flexDirection = 'column';
ReactDOM.createRoot(root).render(<I18nProvider><SettingsFixture/></I18nProvider>);
