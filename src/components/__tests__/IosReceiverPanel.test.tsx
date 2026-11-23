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
        fireEvent.click(screen.getByRole('button',{name:'投屏详情'}));
        expect(screen.getByRole('link',{name:/GPL-3.0 开源接收器/}))
            .toHaveAttribute('href','https://github.com/moieric11/AirPlay-Windows');
        fireEvent.click(screen.getByRole('button',{name:'安装 iOS 接收器'}));
        await waitFor(()=>expect(invokeMock).toHaveBeenCalledWith('ios_airplay_install'));
    });
    it('starts and stops from the homepage without claiming a connected sender',async()=>{
        let current={...ready,installed:true,receiverName:'书房 iPad',receiverIp:'192.168.2.107',receiverReady:true};
        invokeMock.mockImplementation(async(name:string)=>{
            if(name==='ios_airplay_start')current={...current,running:true};
            if(name==='ios_airplay_stop')current={...current,running:false};
            return current;
        });
        render(<IosReceiverPanel locale="zh-CN"/>);
        fireEvent.click(await screen.findByRole('button',{name:'启动 iOS 投屏'}));
        await waitFor(()=>expect(invokeMock).toHaveBeenCalledWith('ios_airplay_start'));
        expect(await screen.findByText(/等待手机连接/)).toBeInTheDocument();
        expect(screen.queryByText('WiFi连接',{exact:true})).not.toBeInTheDocument();
        expect(screen.getByTestId('ios-connection-info')).toHaveTextContent('书房 iPad');
        expect(screen.getByTestId('ios-connection-info')).toHaveTextContent('192.168.2.107');
        expect(screen.getByTestId('ios-connection-info')).toHaveTextContent('控制中心 → 屏幕镜像');
        expect(screen.queryByRole('button',{name:'全屏'})).not.toBeInTheDocument();
        expect(screen.queryByText('iPhone 已连接')).not.toBeInTheDocument();
        fireEvent.click(screen.getByRole('button',{name:'停止接收'}));
        await waitFor(()=>expect(invokeMock).toHaveBeenCalledWith('ios_airplay_stop'));
        await waitFor(()=>expect(screen.queryByText('192.168.2.107')).not.toBeInTheDocument());
        expect(screen.getByRole('button',{name:'编辑接收器名称'})).toBeEnabled();
    });
    it('edits the advertised name beside the connection information',async()=>{
        let current={...ready,installed:true,running:true,receiverReady:true,receiverIp:'192.168.2.107'};
        invokeMock.mockImplementation(async(name:string,args?:{name:string})=>{
            if(name==='ios_airplay_set_receiver_name' && args)current={...current,receiverName:args.name};
            return current;
        });
        render(<IosReceiverPanel locale="zh-CN"/>);
        fireEvent.click(await screen.findByRole('button',{name:'编辑接收器名称'}));
        const input=screen.getByRole('textbox',{name:'接收器名称'});
        expect(input).toHaveValue('AirPlay-Windows');
        fireEvent.change(input,{target:{value:'书'.repeat(17)}});
        fireEvent.click(screen.getByRole('button',{name:'保存'}));
        expect(screen.getByRole('alert')).toHaveTextContent('最多 50 个 UTF-8 字节');
        expect(invokeMock).not.toHaveBeenCalledWith('ios_airplay_set_receiver_name',expect.anything());
        fireEvent.change(input,{target:{value:'  书房 iPad  '}});
        fireEvent.click(screen.getByRole('button',{name:'保存'}));
        await waitFor(()=>expect(invokeMock).toHaveBeenCalledWith('ios_airplay_set_receiver_name',{name:'书房 iPad'}));
        expect(await screen.findByText('书房 iPad',{exact:true})).toBeVisible();
        expect(screen.getByTestId('ios-connection-info')).toHaveTextContent('192.168.2.107');
        expect(screen.queryByRole('textbox',{name:'接收器名称'})).not.toBeInTheDocument();
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
        fireEvent.click(screen.getByRole('button',{name:'投屏详情'}));
        expect(screen.getByText(/实际窗口像素/)).toBeVisible();
    });
    it('updates window sizing live and reports that source quality waits for a restart',async()=>{
        let current={...ready,installed:true,running:true,sourceWidth:1640,sourceHeight:2360};
        invokeMock.mockImplementation(async(name:string,args?:{style?:string;tier?:string;size?:string})=>{
            if(name==='ios_airplay_set_window_style')current={...current,windowStyle:args?.style};
            if(name==='ios_airplay_set_quality_tier') current={...current,qualityTier:args?.tier,qualityPending:true};
            if(name==='ios_airplay_set_window_size') current={...current,windowSize:args?.size};
            return current;
        });
        render(<IosReceiverPanel locale="zh-CN"/>);
        const style=await screen.findByRole('combobox',{name:'iOS 投屏窗口风格'});
        const quality=screen.getByRole('combobox',{name:'iOS 画质'});
        expect(quality).toBeEnabled();
        fireEvent.change(quality,{target:{value:'ultra'}});
        expect(await screen.findByText(/重新启动接收器后/)).toBeInTheDocument();
        fireEvent.change(screen.getByRole('combobox',{name:'iOS 投屏窗口大小'}),{target:{value:'source'}});
        await waitFor(()=>expect(invokeMock).toHaveBeenCalledWith('ios_airplay_set_window_size',{size:'source'}));
        await waitFor(()=>expect(screen.getByRole('combobox',{name:'iOS 投屏窗口大小'})).toHaveValue('source'));
        expect(screen.getByRole('option',{name:'接收器原版界面'})).toBeDisabled();
        fireEvent.change(style,{target:{value:'clean'}});
        await waitFor(()=>expect(invokeMock)
            .toHaveBeenCalledWith('ios_airplay_set_window_style',{style:'clean'}));
        fireEvent.click(screen.getByRole('button',{name:'全屏'}));
        await waitFor(()=>expect(invokeMock)
            .toHaveBeenCalledWith('ios_airplay_window_action',{action:'fullscreen'}));
    });
    it('waits for a USB cable before enabling the header action',async()=>{
        invokeMock.mockResolvedValue({...ready,usbStatus:'waiting'});
        render(<IosReceiverPanel locale="zh-CN"/>);
        expect(await screen.findByText('USB未连接',{exact:true})).toBeVisible();
        expect(screen.getByText('请插入 USB 数据线。')).not.toBeVisible();
        expect(screen.getByRole('button',{name:'USB投屏'})).toBeDisabled();
        expect(invokeMock).not.toHaveBeenCalledWith('ios_usb_control',expect.anything());
    });
    it.each([
        ['detecting','识别设备中'],
        ['unlock','请解锁设备'],
        ['authorization_pending','待确认授权'],
    ])('shows a precise USB notice for %s instead of asking to trust again',async(usbStatus,notice)=>{
        invokeMock.mockResolvedValue({...ready,usbStatus});
        render(<IosReceiverPanel locale="zh-CN"/>);
        expect(await screen.findByText('USB已连接',{exact:true})).toBeVisible();
        expect(screen.getByText(notice,{exact:false})).toBeVisible();
        expect(screen.queryByText('请信任此电脑',{exact:false})).not.toBeInTheDocument();
        expect(screen.getByRole('button',{name:'USB投屏'})).toBeDisabled();
    });
    it('starts USB only on click and offers Stop beside the wireless action',async()=>{
        let usbStatus='ready';
        invokeMock.mockImplementation(async(name:string,args?:{start:boolean})=>{
            if(name==='ios_usb_control')usbStatus=args?.start?'streaming':'ready';
            return {...ready,installed:true,usbStatus,usbSourceWidth:1640,usbSourceHeight:2360};
        });
        render(<IosReceiverPanel locale="zh-CN"/>);
        const usb=await screen.findByRole('button',{name:'USB投屏'});
        const wireless=screen.getByRole('button',{name:'启动 iOS 投屏'});
        expect(usb.nextElementSibling).toBe(wireless);
        expect(screen.getByText('USB已连接',{exact:true})).toBeVisible();
        expect(invokeMock).not.toHaveBeenCalledWith('ios_usb_control',expect.anything());
        fireEvent.click(usb);
        expect(await screen.findByText('USB连接',{exact:true})).toBeVisible();
        expect(screen.queryByText('WiFi连接',{exact:true})).not.toBeInTheDocument();
        const source=await screen.findByText('USB 视频源 1640 × 2360');
        expect(source).not.toBeVisible();
        fireEvent.click(screen.getByRole('button',{name:'投屏详情'}));
        expect(source).toBeVisible();
        fireEvent.click(screen.getByRole('button',{name:'停止 USB 投屏'}));
        expect(await screen.findByRole('button',{name:'USB投屏'})).toBeEnabled();
        expect(invokeMock).toHaveBeenCalledWith('ios_usb_control',{start:true});
        expect(invokeMock).toHaveBeenCalledWith('ios_usb_control',{start:false});
    });
    it('offers the one-time driver approval only if the connected USB device needs it',async()=>{
        invokeMock.mockResolvedValue({...ready,usbStatus:'capture_driver'});
        render(<IosReceiverPanel locale="zh-CN"/>);
        expect(await screen.findByText('需安装驱动',{exact:false})).toBeVisible();
        expect(screen.queryByRole('button',{name:'安装 USB 驱动'})).not.toBeInTheDocument();
        fireEvent.click(screen.getByRole('button',{name:'投屏详情'}));
        fireEvent.click(screen.getByRole('button',{name:'安装 USB 驱动'}));
        expect(await screen.findByText('驱动已安装。请重新插拔数据线。')).toBeVisible();
        expect(invokeMock).toHaveBeenCalledWith('ios_usb_install_driver');
    });
    it('can cancel USB recovery and restart a session after an interruption',async()=>{
        let usbStatus='reconnecting';
        invokeMock.mockImplementation(async(name:string,args?:{start:boolean})=>{
            if(name==='ios_usb_control')usbStatus=args?.start?'streaming':'ready';
            return {...ready,installed:true,usbStatus};
        });
        const {unmount}=render(<IosReceiverPanel locale="zh-CN"/>);
        expect(await screen.findByText('USB重连中',{exact:true})).toBeVisible();
        fireEvent.click(screen.getByRole('button',{name:'停止 USB 投屏'}));
        expect(invokeMock).toHaveBeenCalledWith('ios_usb_control',{start:false});
        expect(await screen.findByText('USB已连接',{exact:true})).toBeVisible();
        unmount();
        usbStatus='interrupted';
        render(<IosReceiverPanel locale="zh-CN"/>);
        expect(await screen.findByText('USB投屏已中断',{exact:true})).toBeVisible();
        fireEvent.click(screen.getByRole('button',{name:'USB投屏'}));
        expect(invokeMock).toHaveBeenCalledWith('ios_usb_control',{start:true});
        expect(await screen.findByText('USB连接',{exact:true})).toBeVisible();
    });
    it('shows actual decoded pixels and warns only when enlarging the sender image',async()=>{
        invokeMock.mockResolvedValue({
            ...ready,installed:true,running:true,qualityTier:'ultra',
            sourceWidth:1640,sourceHeight:2360,displayWidth:1860,displayHeight:2670,
            frameAttached:true,
        });
        render(<IosReceiverPanel locale="zh-CN"/>);
        const resolution=await screen.findByTestId('ios-actual-resolution');
        expect(resolution).not.toBeVisible();
        fireEvent.click(screen.getByRole('button',{name:'投屏详情'}));
        expect(resolution).toBeVisible();
        expect(resolution).toHaveTextContent('实际解码画面：1640 × 2360');
        expect(screen.getByTestId('ios-actual-resolution'))
            .toHaveTextContent('显示窗口：1860 × 2670');
        expect(screen.getByText(/窗口在放大源画面/)).toBeInTheDocument();
        expect(screen.getByText('已接收镜像视频帧')).toBeInTheDocument();
        expect(screen.getByText('WiFi连接',{exact:true})).toBeVisible();
        expect(screen.getByTestId('ios-frame-attachment'))
            .toHaveTextContent('原生图层已对齐');
        expect(screen.queryByText('iPhone 已连接')).not.toBeInTheDocument();
    });
    it('does not misreport an unattached or incompatible receiver bezel as working',async()=>{
        invokeMock.mockResolvedValue({...ready,installed:true,running:true,frameAttached:false,
            frameError:'Windows 拒绝原生透明图层'});
        render(<IosReceiverPanel locale="zh-CN"/>);
        fireEvent.click(screen.getByRole('button',{name:'投屏详情'}));
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
        fireEvent.click(screen.getByRole('button',{name:'投屏详情'}));
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
    it('keeps connection details collapsed and leaves the USB status visible after closing them',async()=>{
        invokeMock.mockResolvedValue({...ready,usbStatus:'ready'});
        render(<IosReceiverPanel locale="zh-CN"/>);
        const help=screen.getByRole('button',{name:'投屏详情'});
        const connected=await screen.findByText('USB已连接',{exact:true});
        expect(help).toHaveAttribute('aria-expanded','false');
        expect(connected).toBeVisible();
        expect(screen.getByText(/Windows 防火墙/)).not.toBeVisible();
        fireEvent.click(help);
        expect(help).toHaveAttribute('aria-expanded','true');
        expect(screen.getByText(/Windows 防火墙/)).toBeVisible();
        expect(screen.getByText(/DRM/)).toBeVisible();
        fireEvent.click(help);
        expect(help).toHaveAttribute('aria-expanded','false');
        expect(screen.getByText(/DRM/)).not.toBeVisible();
        expect(connected).toBeVisible();
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
