import {render,screen,fireEvent} from '@testing-library/react';
import {describe,it,expect,vi} from 'vitest';
import LivePerformance from '../LivePerformance';
import type {ScrcpyConfig, MirrorPerformance, VideoDecoderSupport} from '../../hooks/useScrcpy';

const config:ScrcpyConfig={device:'phone1',sessionMode:'mirror',res:'1920',
    bitrate:8,fps:60,codec:'h264',printFps:true,autoQuality:false};
const support:VideoDecoderSupport={hostOs:'windows',scrcpyVersion:'5.0.1',
    supportsHwdec:true,modes:['auto'],supportsPrintFps:true,diagnostics:''};
const sample:MirrorPerformance={fps:58,skipped:2,observedAt:Date.now(),
    samples:[{fps:58,skipped:2,at:Date.now()-1000},{fps:59,skipped:0,at:Date.now()}]};
describe('live quality monitor',()=>{
    it('does not mistake a configured 60fps cap for measured FPS',()=>{
        render(<LivePerformance locale="zh-CN" device="phone1" running={true}
            config={config} setConfig={vi.fn()} support={support}/>);
        expect(screen.getByTestId('measured-fps')).toHaveTextContent('—');
        expect(screen.getByText(/等待 scrcpy 输出实测帧率/)).toBeInTheDocument();
    });
    it('shows actual FPS, renderer skips and target bitrate distinctly',()=>{
        render(<LivePerformance locale="zh-CN" device="phone1" running={true}
            config={config} setConfig={vi.fn()} support={support}
            performance={sample} decoder="d3d11va"/>);
        expect(screen.getByTestId('measured-fps')).toHaveTextContent('58.0');
        expect(screen.getByTestId('measured-skipped')).toHaveTextContent('2');
        expect(screen.getByText('d3d11va')).toBeInTheDocument();
        expect(screen.getByText(/设定码率/)).toBeInTheDocument();
    });
    it('auto is explicit opt-in and enables measured FPS reporting',()=>{
        const setConfig=vi.fn();
        render(<LivePerformance locale="zh-CN" device="phone1" running={true}
            config={{...config,printFps:false}} setConfig={setConfig} support={support}/>);
        fireEvent.click(screen.getByRole('checkbox',{name:'智能画质 Auto 模式'}));
        expect(setConfig).toHaveBeenCalledWith(expect.objectContaining({autoQuality:true,printFps:true}));
    });
    it('never claims an obsolete disconnected measurement',()=>{
        render(<LivePerformance locale="zh-CN" device="phone1" running={false}
            config={config} setConfig={vi.fn()} support={support} performance={sample}/>);
        expect(screen.getByTestId('measured-fps')).toHaveTextContent('—');
    });
});
