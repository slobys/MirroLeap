import {fireEvent,render,screen,waitFor} from '@testing-library/react';
import {beforeEach,describe,expect,it,vi} from 'vitest';
import IosReceiverPanel,{type IosAirplayStatus} from '../IosReceiverPanel';

const {invokeMock}=vi.hoisted(()=>({invokeMock:vi.fn()}));
vi.mock('@tauri-apps/api/core',()=>({invoke:invokeMock}));
const ready:IosAirplayStatus={
    supported:true,installed:false,running:false,installing:false,
    receiverName:'AirPlay-Windows',engineVersion:'v0.4.1',
    compactMode:true,windowStyle:'device-auto',qualityTier:'high',
};

describe('single-page iOS AirPlay experience',()=>{
    beforeEach(()=>invokeMock.mockReset());

    it('does not open an intermediate dialog or launch/install without a click',async()=>{
        invokeMock.mockResolvedValue(ready);
        render(<IosReceiverPanel locale="zh-CN"/>);
        expect(screen.getByTestId('ios-home-panel')).toBeInTheDocument();
        expect(screen.queryByRole('dialog')).not.toBeInTheDocument();
        await waitFor(()=>expect(invokeMock).toHaveBeenCalledWith('ios_airplay_status'));
        expect(invokeMock).not.toHaveBeenCalledWith('ios_airplay_install');
        expect(invokeMock).not.toHaveBeenCalledWith('ios_airplay_start');
        expect(screen.getByRole('link',{name:/GPL-3.0 开源接收器/}))
            .toHaveAttribute('href','https://github.com/moieric11/AirPlay-Windows');
        fireEvent.click(screen.getByRole('button',{name:'安装 iOS 接收器'}));
        await waitFor(()=>expect(invokeMock).toHaveBeenCalledWith('ios_airplay_install'));
    });
    it('starts and stops from the homepage without claiming a connected sender',async()=>{
        let current={...ready,installed:true};
        invokeMock.mockImplementation(async(name:string)=>{
            if(name==='ios_airplay_start')current={...current,running:true};
            if(name==='ios_airplay_stop')current={...current,running:false};
            return current;
        });
        render(<IosReceiverPanel locale="zh-CN"/>);
        fireEvent.click(await screen.findByRole('button',{name:'启动 iOS 投屏'}));
        await waitFor(()=>expect(invokeMock).toHaveBeenCalledWith('ios_airplay_start'));
        expect(await screen.findByText(/等待手机连接/)).toBeInTheDocument();
        expect(screen.queryByText('iPhone 已连接')).not.toBeInTheDocument();
        fireEvent.click(screen.getByRole('button',{name:'停止接收'}));
        await waitFor(()=>expect(invokeMock).toHaveBeenCalledWith('ios_airplay_stop'));
    });
    it('offers separate real-device frame, clean, classic and resolution choices',async()=>{
        let current={...ready,installed:true};
        invokeMock.mockImplementation(async(name:string,args?:{style?:string;tier?:string})=>{
            if(name==='ios_airplay_set_window_style')current={...current,windowStyle:args?.style};
            if(name==='ios_airplay_set_quality_tier')current={...current,qualityTier:args?.tier};
            return current;
        });
        render(<IosReceiverPanel locale="zh-CN"/>);
        const style=await screen.findByRole('combobox',{name:'iOS 投屏窗口风格'});
        expect(style).toHaveValue('device-auto');
        expect(screen.getByRole('option',{name:'iPad 真机边框'})).toBeInTheDocument();
        expect(screen.getByRole('option',{name:'iPhone 真机边框'})).toBeInTheDocument();
        fireEvent.change(style,{target:{value:'device-ipad'}});
        await waitFor(()=>expect(invokeMock)
            .toHaveBeenCalledWith('ios_airplay_set_window_style',{style:'device-ipad'}));
        const quality=screen.getByRole('combobox',{name:'iOS 画质'});
        expect(quality).toHaveValue('high');
        fireEvent.change(quality,{target:{value:'ultra'}});
        await waitFor(()=>expect(invokeMock)
            .toHaveBeenCalledWith('ios_airplay_set_quality_tier',{tier:'ultra'}));
    });
    it('offers a window-matched source cap instead of relying only on maximum 4K',async()=>{
        let current={...ready,installed:true};
        invokeMock.mockImplementation(async(name:string,args?:{tier?:string})=>{
            if(name==='ios_airplay_set_quality_tier')current={...current,qualityTier:args?.tier};
            return current;
        });
        render(<IosReceiverPanel locale="zh-CN"/>);
        const quality=await screen.findByRole('combobox',{name:'iOS 画质'});
        expect(screen.getByRole('option',{name:/智能清晰/})).toBeInTheDocument();
        fireEvent.change(quality,{target:{value:'adaptive'}});
        await waitFor(()=>expect(invokeMock).toHaveBeenCalledWith(
            'ios_airplay_set_quality_tier',{tier:'adaptive'}));
        expect(screen.getByText(/实际窗口像素/)).toBeInTheDocument();
    });
    it('allows switching device and clean styles live but blocks unsafe quality changes',async()=>{
        let current={...ready,installed:true,running:true};
        invokeMock.mockImplementation(async(name:string,args?:{style?:string})=>{
            if(name==='ios_airplay_set_window_style')current={...current,windowStyle:args?.style};
            return current;
        });
        render(<IosReceiverPanel locale="zh-CN"/>);
        const style=await screen.findByRole('combobox',{name:'iOS 投屏窗口风格'});
        expect(screen.getByRole('combobox',{name:'iOS 画质'})).toBeDisabled();
        expect(screen.getByRole('option',{name:'接收器原版界面'})).toBeDisabled();
        fireEvent.change(style,{target:{value:'clean'}});
        await waitFor(()=>expect(invokeMock)
            .toHaveBeenCalledWith('ios_airplay_set_window_style',{style:'clean'}));
        fireEvent.click(screen.getByRole('button',{name:'全屏'}));
        await waitFor(()=>expect(invokeMock)
            .toHaveBeenCalledWith('ios_airplay_window_action',{action:'fullscreen'}));
    });
    it('shows actual decoded pixels and warns only when enlarging the sender image',async()=>{
        invokeMock.mockResolvedValue({
            ...ready,installed:true,running:true,qualityTier:'ultra',
            sourceWidth:1640,sourceHeight:2360,displayWidth:1860,displayHeight:2670,
            frameAttached:true,
        });
        render(<IosReceiverPanel locale="zh-CN"/>);
        expect(await screen.findByTestId('ios-actual-resolution'))
            .toHaveTextContent('实际解码画面：1640 × 2360');
        expect(screen.getByTestId('ios-actual-resolution'))
            .toHaveTextContent('显示窗口：1860 × 2670');
        expect(screen.getByText(/窗口在放大源画面/)).toBeInTheDocument();
        expect(screen.getByText('已接收镜像视频帧')).toBeInTheDocument();
        expect(screen.getByTestId('ios-frame-attachment'))
            .toHaveTextContent('原生图层已对齐');
        expect(screen.queryByText('iPhone 已连接')).not.toBeInTheDocument();
    });
    it('does not misreport an unattached or incompatible receiver bezel as working',async()=>{
        invokeMock.mockResolvedValue({...ready,installed:true,running:true,frameAttached:false,
            frameError:'Windows 拒绝原生透明图层'});
        render(<IosReceiverPanel locale="zh-CN"/>);
        expect(await screen.findByTestId('ios-frame-attachment'))
            .toHaveTextContent('原生图层未显示');
        expect(screen.getByTestId('ios-frame-error'))
            .toHaveTextContent('Windows 拒绝原生透明图层');
        expect(screen.getByText(/关闭 MirroLeap 主窗口也会停止接收器/))
            .toBeInTheDocument();
    });
    it('warns when a 4K mirror is downscaled by more than 1.7x',async()=>{
        invokeMock.mockResolvedValue({
            ...ready,installed:true,running:true,qualityTier:'ultra',
            sourceWidth:2668,sourceHeight:3840,
            displayWidth:1272,displayHeight:1830,frameAttached:true,
        });
        render(<IosReceiverPanel locale="zh-CN"/>);
        expect(await screen.findByTestId('ios-downscale-warning'))
            .toHaveTextContent('缩小超过 1.7 倍');
        expect(screen.getByTestId('ios-downscale-warning'))
            .toHaveTextContent('高清 2868');
    });
    it('offers Stop for a previously running receiver without launching a duplicate',async()=>{
        const running={...ready,installed:true,running:true,frameAttached:false};
        const stopped={...running,running:false};
        invokeMock.mockImplementation(async(name:string)=>
            name==='ios_airplay_stop'?stopped:running);
        render(<IosReceiverPanel locale="zh-CN"/>);
        fireEvent.click(await screen.findByRole('button',{name:'停止接收'}));
        await waitFor(()=>expect(invokeMock).toHaveBeenCalledWith('ios_airplay_stop'));
        expect(invokeMock).not.toHaveBeenCalledWith('ios_airplay_start');
    });
    it('keeps connection help collapsed, then shows firewall and DRM limits',async()=>{
        invokeMock.mockResolvedValue(ready);
        render(<IosReceiverPanel locale="zh-CN"/>);
        const help=screen.getByRole('button',{name:'连接说明'});
        expect(help).toHaveAttribute('aria-expanded','false');
        expect(screen.queryByText(/Windows 防火墙/)).not.toBeInTheDocument();
        fireEvent.click(help);
        expect(help).toHaveAttribute('aria-expanded','true');
        expect(screen.getByText(/Windows 防火墙/)).toBeInTheDocument();
        expect(screen.getByText(/DRM/)).toBeInTheDocument();
    });
    it('shows unsupported hosts and component verification errors without false readiness',async()=>{
        invokeMock.mockResolvedValueOnce({...ready,supported:false});
        const {unmount}=render(<IosReceiverPanel locale="zh-CN"/>);
        expect(await screen.findByText('仅支持 Windows x64')).toBeInTheDocument();
        expect(screen.queryByRole('button',{name:'安装 iOS 接收器'})).not.toBeInTheDocument();
        unmount();
        invokeMock.mockImplementation(async(name:string)=>{
            if(name==='ios_airplay_install')throw Error('SHA-256 mismatch');
            return ready;
        });
        render(<IosReceiverPanel locale="zh-CN"/>);
        fireEvent.click(await screen.findByRole('button',{name:'安装 iOS 接收器'}));
        expect(await screen.findByRole('alert')).toHaveTextContent('SHA-256 mismatch');
    });
});
