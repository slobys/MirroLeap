import { fireEvent, render, screen, waitFor } from '@testing-library/react';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import CompactDashboard from '../CompactDashboard';
import type { ScrcpyConfig } from '../../hooks/useScrcpy';
const {invokeMock}=vi.hoisted(()=>({invokeMock:vi.fn()}));
vi.mock('@tauri-apps/api/core',()=>({invoke:invokeMock}));

const selected = '192.168.2.226:5555';
const config: ScrcpyConfig = {
    device: selected, sessionMode: 'mirror', res: '0',
    deviceFrame: true, audioEnabled: true,
};
function props(overrides: Partial<React.ComponentProps<typeof CompactDashboard>> = {}) {
    return {
        locale: 'zh-CN', version: '4.6.0', hasUpdate:false, onOpenUpdate:vi.fn(),
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
    beforeEach(()=>invokeMock.mockReset().mockResolvedValue(undefined));
    afterEach(()=>{delete (window as unknown as {__TAURI_INTERNALS__?:unknown}).__TAURI_INTERNALS__;});
    it('opens YouTube and Telegram using an allowlisted Rust OS browser handoff',async()=>{
        Object.defineProperty(window,'__TAURI_INTERNALS__',{value:{},configurable:true});
        render(<CompactDashboard {...props()}/>);
        fireEvent.click(screen.getByRole('link',{name:'奶油 YouTube 频道'}));
        fireEvent.click(screen.getByRole('link',{name:'奶油 Telegram 群组'}));
        await waitFor(()=>expect(invokeMock).toHaveBeenCalledTimes(2));
        expect(invokeMock).toHaveBeenNthCalledWith(1,'open_official_community_link',{destination:'youtube'});
        expect(invokeMock).toHaveBeenNthCalledWith(2,'open_official_community_link',{destination:'telegram'});
    });
    it('shows an explicit error if Windows has no default browser',async()=>{
        Object.defineProperty(window,'__TAURI_INTERNALS__',{value:{},configurable:true});
        invokeMock.mockRejectedValueOnce(new Error('No browser'));
        render(<CompactDashboard {...props()}/>);
        fireEvent.click(screen.getByRole('link',{name:'奶油 YouTube 频道'}));
        await waitFor(()=>expect(screen.getByRole('alert')).toHaveTextContent('无法打开外部浏览器'));
    });
    it('shows only the common actions and keeps advanced controls behind Settings', () => {
        const args = props();
        render(<CompactDashboard {...args}/>);
        expect(screen.getByRole('heading', {name: 'MirroLeap'})).toBeInTheDocument();
        expect(screen.getByRole('heading', {name: '我的设备'})).toBeInTheDocument();
        expect(screen.getByRole('button', {name: '开始投屏'})).toBeEnabled();
        expect(screen.getByRole('combobox', {name: '分辨率'})).toBeInTheDocument();
        expect(screen.queryByText('图形渲染器')).not.toBeInTheDocument();
        expect(screen.queryByText('系统控制台')).not.toBeInTheDocument();
        expect(screen.getByRole('link',{name:'奶油 YouTube 频道'}))
            .toHaveAttribute('href','https://www.youtube.com/@naiyou');
        expect(screen.getByRole('link',{name:'奶油 Telegram 群组'}))
            .toHaveAttribute('href','https://t.me/naiyous');
        fireEvent.click(screen.getByRole('button', {name:'打开设置'}));
        expect(args.onOpenSettings).toHaveBeenCalledWith('mirror');
    });
    it('offers a shared version button with a green dot only when an update exists', () => {
        const args=props({hasUpdate:true});
        const {container,rerender}=render(<CompactDashboard {...args}/>);
        expect(container.querySelector('.ml-version-dot')).not.toBeNull();
        fireEvent.click(screen.getByRole('button',{name:/v4\.6\.0.*发现新版/}));
        expect(args.onOpenUpdate).toHaveBeenCalledOnce();
        rerender(<CompactDashboard {...props({hasUpdate:false})}/>);
        expect(container.querySelector('.ml-version-dot')).toBeNull();
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
    it('shows only one resolution dropdown and retains the detailed 480p video settings line', () => {
        const args=props({config:{...config,res:'800',bitrate:3,fps:60,codec:'h264'}});
        render(<CompactDashboard {...args}/>);
        expect(screen.queryByRole('combobox',{name:'场景预设'})).not.toBeInTheDocument();
        expect(screen.getAllByRole('combobox')).toHaveLength(1);
        const selector = screen.getByRole('combobox',{name:'分辨率'});
        expect(selector).toHaveValue('800');
        expect(screen.queryByRole('option',{name:/原始分辨率/})).not.toBeInTheDocument();
        expect(screen.getByTestId('home-video-summary')).toHaveTextContent(
            '当前参数：480p · 3Mbps · 最高 60 帧 · H264'
        );
        for (const value of ['1280','1920','2560','3840']) {
            expect(screen.getByRole('option',{name:new RegExp(value === '1280' ? '720p' :
                value === '1920' ? '1080P' : value === '2560' ? '2K' : '4K')})).toBeInTheDocument();
        }
        fireEvent.change(selector,{target:{value:'2560'}});
        expect(args.onResolutionChange).toHaveBeenCalledExactlyOnceWith('2560');
        expect(args.onStart).not.toHaveBeenCalled();
        expect(args.onStop).not.toHaveBeenCalled();
    });
    it('routes a running 720p resolution selection through automatic video switching', () => {
        const args=props({runningDevices:[selected]});
        render(<CompactDashboard {...args}/>);
        expect(screen.getByText(/切换分辨率将自动匹配画质参数/)).toBeInTheDocument();
        fireEvent.change(screen.getByRole('combobox',{name:'分辨率'}),
            {target:{value:'1280'}});
        expect(args.onResolutionChange).toHaveBeenCalledExactlyOnceWith('1280');
        expect(args.onStop).not.toHaveBeenCalled();
        expect(args.onStart).not.toHaveBeenCalled();
    });
    it('shows the current 2K parameters and updates the description for the selected config', () => {
        const initial = props({config:{...config,res:'2560',bitrate:16,fps:60,codec:'h264'}});
        const {rerender}=render(<CompactDashboard {...initial}/>);
        expect(screen.getByTestId('home-video-summary')).toHaveTextContent(
            '当前参数：2K · 16Mbps · 最高 60 帧 · H264'
        );
        expect(screen.getByText(/2K 已可流畅使用/)).toBeInTheDocument();
        rerender(<CompactDashboard {...props({config:{...config,res:'3840',bitrate:20,fps:30,codec:'h264'}})}/>);
        expect(screen.getByTestId('home-video-summary')).toHaveTextContent(
            '当前参数：4K · 20Mbps · 最高 30 帧 · H264'
        );
    });

    it('keeps switching indicator in place and disables manual stop until handoff finishes', () => {
        const args=props({runningDevices:[],isSwitchingResolution:true});
        render(<CompactDashboard {...args}/>);
        const stop=screen.getByRole('button',{name:'正在切换画质…'});
        expect(stop).toBeDisabled();
        expect(screen.queryByRole('button',{name:'开始投屏'})).not.toBeInTheDocument();
        expect(screen.getByText(/正在自动应用画质，Wi-Fi 配对保留/)).toBeInTheDocument();
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
