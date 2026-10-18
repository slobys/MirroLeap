import { fireEvent, render, screen } from '@testing-library/react';
import { describe, expect, it, vi } from 'vitest';
import Header from '../Header';
import SessionBehavior from '../SessionBehavior';
import type { ScrcpyConfig } from '../../hooks/useScrcpy';

describe('redesigned settings center keeps existing actions accessible', () => {
  it('keeps theme, color mode, engine selection and manual help functional', () => {
    const onThemeChange = vi.fn();
    const onColorModeChange = vi.fn();
    const onSetPath = vi.fn();
    const onResetPath = vi.fn();
    const onDownload = vi.fn();
    render(<Header
      currentTheme="ultraviolet"
      onThemeChange={onThemeChange}
      colorMode="light"
      onColorModeChange={onColorModeChange}
      binaryStatus={{found:false,message:'Engine not found'}}
      isDownloading={false} downloadProgress={0} version="5.0.8"
      onDownload={onDownload} onSetPath={onSetPath} onResetPath={onResetPath}
    />);
    fireEvent.click(screen.getByRole('button', {name:'Astro Blue'}));
    expect(onThemeChange).toHaveBeenCalledWith('astro');
    fireEvent.click(screen.getByRole('button', {name:'Dark'}));
    expect(onColorModeChange).toHaveBeenCalledWith('dark');
    fireEvent.click(screen.getByRole('button', {name:'Select Folder'}));
    expect(onSetPath).toHaveBeenCalledOnce();
    fireEvent.click(screen.getByRole('button', {name:'Reset Path'}));
    expect(onResetPath).toHaveBeenCalledOnce();
    fireEvent.click(screen.getByRole('button', {name:'Install Core'}));
    expect(onDownload).toHaveBeenCalledOnce();
    fireEvent.click(screen.getByRole('button', {name:'Setup Help'}));
    expect(screen.getByRole('dialog', {name:'Manual Setup Guide'})).toBeInTheDocument();
  });

  it('keeps grouped device and recording options keyboard operable', () => {
    const config: ScrcpyConfig = {device:'device-1',sessionMode:'mirror',res:'1920'};
    const setConfig = vi.fn();
    render(<SessionBehavior config={config} setConfig={setConfig}/>);
    expect(screen.getByText('Device & connection')).toBeInTheDocument();
    expect(screen.queryByText('Window & display')).not.toBeInTheDocument();
    expect(screen.getByText('Recording & saving')).toBeInTheDocument();
    const toggle = screen.getByRole('switch', {name:'Stay Awake'});
    expect(toggle).toHaveAttribute('aria-checked','false');
    fireEvent.keyDown(toggle, {key:'Enter'});
    expect(setConfig).toHaveBeenCalledWith(expect.objectContaining({stayAwake:true}));
    fireEvent.click(screen.getByRole('switch', {name:'Record Feed'}));
    expect(setConfig).toHaveBeenCalledWith(expect.objectContaining({record:true}));
  });

  it('offers an explicit restart for recording changes during a mirror and reports the saved file', async () => {
    const config: ScrcpyConfig = {
      device:'device-1',sessionMode:'mirror',res:'0',record:true,recordPath:'C:\\Users\\tester\\Videos'
    };
    const onApplyRecordingNow=vi.fn().mockResolvedValue(true);
    render(<SessionBehavior config={config} setConfig={vi.fn()}
      isRunning activeRecording={{record:false,recordPath:'C:\\Users\\tester\\Videos'}}
      onApplyRecordingNow={onApplyRecordingNow}
      recordingStatus={{device:'device-1',phase:'saved',path:'C:\\Users\\tester\\Videos\\MirroLeap_record.mkv'}}/>);
    const apply=screen.getByRole('button',{name:'Apply recording settings and restart mirroring'});
    expect(apply).toBeEnabled();
    fireEvent.click(apply);
    expect(onApplyRecordingNow).toHaveBeenCalledTimes(1);
    expect(screen.getByText('Recording file saved')).toBeInTheDocument();
    expect(screen.getByText(/MirroLeap_record.mkv/)).toBeInTheDocument();
  });

  it('explains that a record checkbox applies when a new stream starts', () => {
    const config:ScrcpyConfig={device:'device-1',sessionMode:'mirror',record:true};
    render(<SessionBehavior config={config} setConfig={vi.fn()}/>);
    expect(screen.getByText(/Start mirroring to record/)).toBeInTheDocument();
  });
});
