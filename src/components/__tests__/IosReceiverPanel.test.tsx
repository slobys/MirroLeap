import {render,screen,fireEvent,waitFor} from '@testing-library/react';
import {beforeEach,describe,expect,it,vi} from 'vitest';
import IosReceiverPanel, {type IosAirplayStatus} from '../IosReceiverPanel';

const {invokeMock}=vi.hoisted(()=>({invokeMock:vi.fn()}));
vi.mock('@tauri-apps/api/core',()=>({invoke:invokeMock}));
const ready: IosAirplayStatus={
    supported:true,installed:false,running:false,installing:false,
    engineVersion:'v0.4.1',receiverName:'AirPlay-Windows',compactMode:true,
};
describe('opt-in iOS AirPlay receiver in MirroLeap',()=>{
    beforeEach(()=>invokeMock.mockReset());
    it('does not download AirPlay until an explicit click and links to GPL source',async()=>{
        invokeMock.mockResolvedValue(ready);
        render(<IosReceiverPanel locale="zh-CN" onClose={vi.fn()}/>);
        expect(await screen.findByText('尚未安装 AirPlay 接收组件')).toBeInTheDocument();
        expect(invokeMock).toHaveBeenCalledWith('ios_airplay_status');
        expect(invokeMock).not.toHaveBeenCalledWith('ios_airplay_install');
        expect(screen.getByRole('link',{name:/源码与许可证/})).toHaveAttribute(
            'href','https://github.com/moieric11/AirPlay-Windows');
        expect(screen.getByText(/开源 GPL-3.0/)).toBeInTheDocument();
        fireEvent.click(screen.getByRole('button',{name:/下载安装 AirPlay 组件/}));
        await waitFor(()=>expect(invokeMock).toHaveBeenCalledWith('ios_airplay_install'));
    });
    it('defaults to clean screen-only layout and only changes next launch on user command',async()=>{
        let state={...ready,installed:true};
        invokeMock.mockImplementation(async(name:string,args?:{compact?:boolean})=>{
            if(name==='ios_airplay_status')return state;
            if(name==='ios_airplay_set_compact_mode'){
                state={...state,compactMode:!!args?.compact};
                return state;
            }
            return state;
        });
        render(<IosReceiverPanel locale="zh-CN" onClose={vi.fn()}/>);
        const clean=await screen.findByRole('button',{name:'简洁投屏（推荐）'});
        const classic=screen.getByRole('button',{name:'原版控制界面'});
        expect(clean).toHaveAttribute('aria-pressed','true');
        expect(invokeMock).not.toHaveBeenCalledWith('ios_airplay_set_compact_mode',expect.anything());
        fireEvent.click(classic);
        await waitFor(()=>expect(invokeMock).toHaveBeenCalledWith('ios_airplay_set_compact_mode',{compact:false}));
        await waitFor(()=>expect(classic).toHaveAttribute('aria-pressed','true'));
        fireEvent.click(clean);
        await waitFor(()=>expect(invokeMock).toHaveBeenCalledWith('ios_airplay_set_compact_mode',{compact:true}));
    });
    it('can request safe window actions without disconnecting a running iPad',async()=>{
        const started={...ready,installed:true,running:true};
        invokeMock.mockImplementation(async(name:string)=>{
            if(name==='ios_airplay_status'||name==='ios_airplay_window_action')return started;
            return started;
        });
        render(<IosReceiverPanel locale="zh-CN" onClose={vi.fn()}/>);
        const shortcut=await screen.findByRole('button',{name:/切换简洁\/原界面/});
        expect(screen.getByRole('button',{name:'简洁投屏（推荐）'})).toBeDisabled();
        fireEvent.click(shortcut);
        await waitFor(()=>expect(invokeMock).toHaveBeenCalledWith('ios_airplay_window_action',{action:'toggle_clean'}));
        expect(await screen.findByText(/已向接收窗口发送 H 切换请求/)).toBeInTheDocument();
        fireEvent.click(screen.getByRole('button',{name:/全屏\/退出/}));
        await waitFor(()=>expect(invokeMock).toHaveBeenCalledWith('ios_airplay_window_action',{action:'fullscreen'}));
        expect(screen.queryByText('iPhone 已连接')).not.toBeInTheDocument();
    });
    it('starts and stops its own process but never reports a connected iPhone',async()=>{
        const installed={...ready,installed:true};
        const running={...installed,running:true};
        invokeMock.mockImplementation(async (name:string)=>{
            if(name==='ios_airplay_status')return installed;
            if(name==='ios_airplay_start')return running;
            if(name==='ios_airplay_stop')return installed;
            return installed;
        });
        render(<IosReceiverPanel locale="zh-CN" onClose={vi.fn()}/>);
        fireEvent.click(await screen.findByRole('button',{name:'启动 iOS 接收器'}));
        expect(await screen.findByText(/等待 iPhone 连接/)).toBeInTheDocument();
        expect(invokeMock).toHaveBeenCalledWith('ios_airplay_start');
        expect(screen.queryByText('iPhone 已连接')).not.toBeInTheDocument();
        fireEvent.click(screen.getByRole('button',{name:'停止 iOS 接收器'}));
        await waitFor(()=>expect(invokeMock).toHaveBeenCalledWith('ios_airplay_stop'));
    });
    it('explains unsupported hosts, sender instructions and protected-content limits',async()=>{
        invokeMock.mockResolvedValue({...ready,supported:false});
        render(<IosReceiverPanel locale="zh-CN" onClose={vi.fn()}/>);
        expect(await screen.findByText('仅支持 Windows x64')).toBeInTheDocument();
        expect(screen.queryByRole('button',{name:'下载安装 AirPlay 组件'})).not.toBeInTheDocument();
        expect(screen.getByText(/屏幕镜像/)).toBeInTheDocument();
        expect(screen.getByText(/DRM/)).toBeInTheDocument();
    });
    it('shows installation errors and never claims the receiver is ready',async()=>{
        invokeMock.mockImplementation(async (name:string)=>{
            if(name==='ios_airplay_status')return ready;
            if(name==='ios_airplay_install')throw Error('SHA-256 mismatch');
        });
        render(<IosReceiverPanel locale="en-US" onClose={vi.fn()}/>);
        fireEvent.click(await screen.findByRole('button',{name:'Install AirPlay component'}));
        expect(await screen.findByRole('alert')).toHaveTextContent('SHA-256 mismatch');
        expect(screen.queryByText(/Receiver running/)).not.toBeInTheDocument();
    });
});
