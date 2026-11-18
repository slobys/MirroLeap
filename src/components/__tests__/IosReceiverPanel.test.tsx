import {render,screen,fireEvent,waitFor} from '@testing-library/react';
import {beforeEach,describe,expect,it,vi} from 'vitest';
import IosReceiverPanel, {type IosAirplayStatus} from '../IosReceiverPanel';

const {invokeMock}=vi.hoisted(()=>({invokeMock:vi.fn()}));
vi.mock('@tauri-apps/api/core',()=>({invoke:invokeMock}));
const ready: IosAirplayStatus={
    supported:true,installed:false,running:false,installing:false,
    engineVersion:'v0.4.1',receiverName:'AirPlay-Windows',
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
