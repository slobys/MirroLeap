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
      isDownloading={false} downloadProgress={0} version="5.0.5"
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
    expect(screen.getByText('Window & display')).toBeInTheDocument();
    expect(screen.getByText('Recording & saving')).toBeInTheDocument();
    const toggle = screen.getByRole('switch', {name:'Stay Awake'});
    expect(toggle).toHaveAttribute('aria-checked','false');
    fireEvent.keyDown(toggle, {key:'Enter'});
    expect(setConfig).toHaveBeenCalledWith(expect.objectContaining({stayAwake:true}));
    fireEvent.click(screen.getByRole('switch', {name:'Record Feed'}));
    expect(setConfig).toHaveBeenCalledWith(expect.objectContaining({record:true}));
  });
});
