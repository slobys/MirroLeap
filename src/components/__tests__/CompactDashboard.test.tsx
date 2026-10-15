import { fireEvent, render, screen } from '@testing-library/react';
import { describe, expect, it, vi } from 'vitest';
import CompactDashboard from '../CompactDashboard';
import type { ScrcpyConfig } from '../../hooks/useScrcpy';

const selected = '192.168.2.226:5555';
const config: ScrcpyConfig = {
    device: selected, sessionMode: 'mirror', res: '0',
    deviceFrame: true, audioEnabled: true,
};
function props(overrides: Partial<React.ComponentProps<typeof CompactDashboard>> = {}) {
    return {
        locale: 'zh-CN', version: '4.6.0',
        devices: [selected, 'usb-device'], pendingDevices: [],
        deviceModels: {}, deviceFriendlyNames: { [selected]: 'Xiaomi 14 Ultra' },
        runningDevices: [], activeDevice: selected,
        onSelectDevice: vi.fn(), onRefresh: vi.fn(), isRefreshing: false,
        config, onConfigChange: vi.fn(), onResolutionChange: vi.fn(),
        isSwitchingResolution: false,
        onStart: vi.fn(), onStop: vi.fn(), onOpenSettings: vi.fn(),
        coreReady: true, onDownloadCore: vi.fn(),
        isDownloading: false, downloadProgress: 0,
        ...overrides,
    } satisfies React.ComponentProps<typeof CompactDashboard>;
}

describe('MirroLeap daily workspace', () => {
    it('shows only the common actions and keeps advanced controls behind Settings', () => {
        const args = props();
        render(<CompactDashboard {...args}/>);
        expect(screen.getByRole('heading', {name: 'MirroLeap'})).toBeInTheDocument();
        expect(screen.getByRole('heading', {name: '我的设备'})).toBeInTheDocument();
        expect(screen.getByRole('button', {name: '开始投屏'})).toBeEnabled();
        expect(screen.getByRole('combobox', {name: '分辨率'})).toBeInTheDocument();
        expect(screen.queryByText('图形渲染器')).not.toBeInTheDocument();
        expect(screen.queryByText('系统控制台')).not.toBeInTheDocument();
        fireEvent.click(screen.getByRole('button', {name:'打开设置'}));
        expect(args.onOpenSettings).toHaveBeenCalledWith('mirror');
    });
    it('allows selecting a device, refreshing and opening Wi-Fi pairing', () => {
        const args = props();
        render(<CompactDashboard {...args}/>);
        fireEvent.click(screen.getByRole('button', {name:/usb-device/}));
        expect(args.onSelectDevice).toHaveBeenCalledWith('usb-device');
        fireEvent.click(screen.getByRole('button', {name:'刷新设备列表'}));
        expect(args.onRefresh).toHaveBeenCalledOnce();
        fireEvent.click(screen.getByRole('button', {name:/无线连接/}));
        expect(args.onOpenSettings).toHaveBeenCalledWith('devices');
    });
    it('updates quality, sound and phone frame through existing callbacks', () => {
        const args = props();
        render(<CompactDashboard {...args}/>);
        fireEvent.change(screen.getByRole('combobox', {name:'分辨率'}), {target:{value:'1280'}});
        expect(args.onResolutionChange).toHaveBeenCalledExactlyOnceWith('1280');
        fireEvent.click(screen.getByRole('button', {name:'声音'}));
        expect(args.onConfigChange).toHaveBeenCalledWith({...config, audioEnabled:false});
        fireEvent.click(screen.getByRole('button', {name:'真机边框'}));
        expect(args.onConfigChange).toHaveBeenCalledWith({...config, deviceFrame:false});
        fireEvent.click(screen.getByRole('button', {name:'开始投屏'}));
        expect(args.onStart).toHaveBeenCalledOnce();
    });
    it('stops a running mirror and disables Start when no device was selected', () => {
        const running = props({runningDevices:[selected]});
        const {rerender} = render(<CompactDashboard {...running}/>);
        fireEvent.click(screen.getByRole('button', {name:'停止投屏'}));
        expect(running.onStop).toHaveBeenCalledOnce();
        rerender(<CompactDashboard {...props({activeDevice:'',devices:[]})}/>);
        expect(screen.getByRole('button', {name:'开始投屏'})).toBeDisabled();
        expect(screen.getByText('还没有连接手机')).toBeInTheDocument();
    });
    it('offers safe core installation instead of pretending the mirror can start', () => {
        const args=props({coreReady:false});
        render(<CompactDashboard {...args}/>);
        expect(screen.queryByRole('button', {name:'开始投屏'})).not.toBeInTheDocument();
        fireEvent.click(screen.getByRole('button', {name:'安装投屏引擎'}));
        expect(args.onDownloadCore).toHaveBeenCalledOnce();
    });
    it('uses one small scenario selector while preserving device and audio settings', () => {
        const args=props();
        render(<CompactDashboard {...args}/>);
        expect(screen.getByRole('combobox', {name:'场景预设'})).toHaveValue('custom');
        fireEvent.change(screen.getByRole('combobox', {name:'场景预设'}),
            {target:{value:'presentation'}});
        expect(args.onConfigChange).toHaveBeenCalledWith({
            ...config, bitrate: 8, fps: 30, res:'1920',
        });
        expect(args.onStart).not.toHaveBeenCalled();
        expect(args.onStop).not.toHaveBeenCalled();
    });
    it('warns when preset will apply only to the next running session', () => {
        const args=props({runningDevices:[selected]});
        render(<CompactDashboard {...args}/>);
        expect(screen.getByText(/下次启动投屏时生效/)).toBeInTheDocument();
        fireEvent.change(screen.getByRole('combobox', {name:'场景预设'}),
            {target:{value:'low-latency'}});
        expect(args.onConfigChange).toHaveBeenCalledWith({
            ...config, bitrate:8, fps:60, res:'1280',
        });
        expect(args.onStop).not.toHaveBeenCalled();
    });
    it('keeps the settings shortcut in a separate semantic footer when running', () => {
        const args = props({runningDevices:[selected]});
        const {container} = render(<CompactDashboard {...args}/>);
        const footer = container.querySelector('footer.ml-home-footer');
        expect(footer).toBeInTheDocument();
        expect(footer?.contains(screen.getByRole('button', {name:/更多功能与快捷键/}))).toBe(true);
        expect(container.querySelector('.ml-mirror-card .ml-stop-action')).toBeInTheDocument();
        expect(footer?.contains(container.querySelector('.ml-mirror-card .ml-stop-action'))).toBe(false);
        fireEvent.click(screen.getByRole('button', {name:/更多功能与快捷键/}));
        expect(args.onOpenSettings).toHaveBeenCalledWith('behavior');
    });
});
