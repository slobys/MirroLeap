import { fireEvent, render, screen, waitFor } from '@testing-library/react';
import { describe, expect, it, vi } from 'vitest';
import DecoderPerformance from '../DecoderPerformance';
import type { ScrcpyConfig, VideoDecoderSupport } from '../../hooks/useScrcpy';
import {invoke} from '@tauri-apps/api/core';

vi.mock('@tauri-apps/api/core',()=>({invoke:vi.fn(async()=>true)}));
vi.mock('@tauri-apps/api/event',()=>({listen:vi.fn(async()=>()=>{})}));


const base: ScrcpyConfig = {
  device: 'emulator-5554', sessionMode: 'mirror', bitrate: 8,
  res: '0', codec: 'h264', audioEnabled: true, hardwareDecoder: 'auto',
  printFps: false, deviceFrame: true, record: true,
};

const windows5: VideoDecoderSupport = {
  hostOs: 'windows', scrcpyVersion:'5.0.1', supportsHwdec:true,
  modes:['auto','disabled','d3d11va'], supportsPrintFps:true,
  diagnostics:'scrcpy --help modes',
};

describe('safe host hardware decoder controls', () => {
  it('does not force hardware decoding for older scrcpy cores', () => {
    const setConfig = vi.fn();
    render(<DecoderPerformance config={base} setConfig={setConfig}
      support={{...windows5, scrcpyVersion:'4.1.0', supportsHwdec:false,modes:[]}}
      running={false}/>);
    expect(screen.getByLabelText('Host video decoder')).toBeDisabled();
    expect(screen.getByText(/requires 5.0/)).toBeInTheDocument();
    expect(screen.getByLabelText(/Log actual FPS/)).toBeEnabled();
    expect(setConfig).not.toHaveBeenCalled();
  });

  it('distinguishes Windows D3D11 GPU vs software and preserves unrelated config fields', () => {
    const setConfig = vi.fn();
    render(<DecoderPerformance config={base} setConfig={setConfig} support={windows5} running={false}/>);
    fireEvent.change(screen.getByLabelText('Host video decoder'), {target:{value:'d3d11va'}});
    expect(setConfig).toHaveBeenCalledWith({...base,hardwareDecoder:'d3d11va'});
    fireEvent.change(screen.getByLabelText('Host video decoder'), {target:{value:'disabled'}});
    expect(setConfig).toHaveBeenLastCalledWith({...base,hardwareDecoder:'disabled'});
    fireEvent.click(screen.getByLabelText(/Log actual FPS/));
    expect(setConfig).toHaveBeenLastCalledWith({...base,printFps:true});
  });

  it('does not claim a GPU is active until scrcpy reports an actual decoder', () => {
    const {rerender} = render(<DecoderPerformance config={{...base,hardwareDecoder:'d3d11va'}}
      setConfig={vi.fn()} support={windows5} running={true}/>);
    expect(screen.getByRole('status')).toHaveTextContent('Not yet reported');
    expect(screen.getByText(/apply to your next mirroring session/)).toBeInTheDocument();
    rerender(<DecoderPerformance config={{...base,hardwareDecoder:'d3d11va'}}
      setConfig={vi.fn()} support={windows5} running={true} observedMode="software"/>);
    expect(screen.getByRole('status')).toHaveTextContent('software');
    rerender(<DecoderPerformance config={{...base,hardwareDecoder:'d3d11va'}}
      setConfig={vi.fn()} support={windows5} running={true} observedMode="retrying"/>);
    expect(screen.getByRole('status')).toHaveTextContent('Retrying compatible decoder');
  });

  it('only downloads optional HQ core after explicit checkbox opt-in', async () => {
    const setConfig=vi.fn();
    const {rerender}=render(<DecoderPerformance config={base} setConfig={setConfig}
      support={windows5} running={false}/>);
    const option=screen.getByLabelText(/Optional 2.35× HQ lens/);
    expect(option).not.toBeChecked();
    fireEvent.click(option);
    await waitFor(()=>expect(invoke).toHaveBeenCalledWith('ensure_hq_runtime'));
    await waitFor(()=>expect(setConfig).toHaveBeenLastCalledWith({...base,hqLens:true}));
    rerender(<DecoderPerformance config={{...base,hqLens:true}} setConfig={setConfig}
      support={windows5} running={true}/>);
    expect(screen.getByText(/CPU\/GDI decoded-frame preview, not final D3D11/)).toBeInTheDocument();
  });

  it('never marks optional HQ active if the verified core download fails', async()=>{
    vi.mocked(invoke).mockRejectedValueOnce('network offline');
    const setConfig=vi.fn();
    render(<DecoderPerformance config={base} setConfig={setConfig}
      support={windows5} running={false}/>);
    fireEvent.click(screen.getByLabelText(/Optional 2.35× HQ lens/));
    await waitFor(()=>expect(screen.getByRole('alert')).toHaveTextContent('network offline'));
    expect(setConfig).not.toHaveBeenCalled();
    expect(screen.getByLabelText(/Optional 2.35× HQ lens/)).not.toBeChecked();
  });

  it('warns about known Windows 5.0.0 H.265 compatibility and falls back on stale OS modes', () => {
    const oldWindows = {...windows5,scrcpyVersion:'5.0.0'};
    render(<DecoderPerformance config={{...base,codec:'h265',hardwareDecoder:'vaapi'}}
      setConfig={vi.fn()} support={oldWindows} running={false}/>);
    expect(screen.getByText(/issues in scrcpy 5.0.0 were fixed in 5.0.1/)).toBeInTheDocument();
    expect(screen.getByText(/saved decoder preference is unavailable/)).toBeInTheDocument();
    expect(screen.getByLabelText('Host video decoder')).toHaveValue('auto');
  });
});
