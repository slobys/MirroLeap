import {beforeEach,describe,expect,it,vi} from 'vitest';
import {fireEvent,render,screen,waitFor} from '@testing-library/react';
import StartupUpdateNotice from '../StartupUpdateNotice';
import type {AppUpdateInfo} from '../../utils/appUpdate';

const {invokeMock,listenMock}=vi.hoisted(()=>({
  invokeMock:vi.fn(),listenMock:vi.fn().mockResolvedValue(()=>{}),
}));
vi.mock('@tauri-apps/api/core',()=>({invoke:invokeMock}));
vi.mock('@tauri-apps/api/event',()=>({listen:listenMock}));

const release:AppUpdateInfo={
  currentVersion:'5.0.7',latestVersion:'5.0.8',status:'update_available',
  prerelease:true,
  releaseUrl:'https://github.com/slobys/MirroLeap/releases/tag/v5.0.8',
  publishedAt:'2026-10-17T09:00:00Z',changelog:'New MirroLeap fixes',
  downloads:[],
};

describe('startup opt-in app update notice',()=>{
  beforeEach(()=>{
    invokeMock.mockReset().mockResolvedValue(undefined);
    listenMock.mockReset().mockResolvedValue(()=>{});
  });
  it('shows version and never installs before user explicitly clicks',()=>{
    render(<StartupUpdateNotice release={release} installKind="installed"
      running={false} onDismiss={vi.fn()}/>);
    expect(screen.getByRole('dialog')).toBeInTheDocument();
    expect(screen.getByText(/v5.0.7 → v5.0.8/)).toBeInTheDocument();
    expect(invokeMock).not.toHaveBeenCalled();
    expect(screen.getByRole('button',{name:'Install verified update'})).toBeEnabled();
  });
  it('uses Rust signature-verified installer only after confirmation',async()=>{
    render(<StartupUpdateNotice release={release} installKind="installed"
      running={false} onDismiss={vi.fn()}/>);
    fireEvent.click(screen.getByRole('button',{name:'Install verified update'}));
    await waitFor(()=>expect(invokeMock).toHaveBeenCalledWith('install_signed_update',{
      version:'5.0.8',
    }));
    expect(await screen.findByText(/verified installer was launched/)).toBeInTheDocument();
  });
  it('does not replace portable/MSI copies; opens a canonical release page',async()=>{
    const onDismiss=vi.fn();
    render(<StartupUpdateNotice release={release} installKind="portable"
      running={false} onDismiss={onDismiss}/>);
    fireEvent.click(screen.getByRole('button',{name:'Open official download'}));
    await waitFor(()=>expect(invokeMock).toHaveBeenCalledWith('open_official_release_link',{
      version:'5.0.8',kind:'page',
    }));
    expect(invokeMock).not.toHaveBeenCalledWith('install_signed_update',expect.anything());
    expect(onDismiss).toHaveBeenCalledOnce();
  });
  it('blocks update while an active mirror is running and supports Later',async()=>{
    const onDismiss=vi.fn();
    render(<StartupUpdateNotice release={release} installKind="installed"
      running onDismiss={onDismiss}/>);
    fireEvent.click(screen.getByRole('button',{name:'Install verified update'}));
    expect(await screen.findByRole('alert')).toHaveTextContent('Stop the active phone mirror');
    expect(invokeMock).not.toHaveBeenCalled();
    fireEvent.click(screen.getByRole('button',{name:'Later'}));
    expect(onDismiss).toHaveBeenCalledOnce();
  });
  it('surfaces signed installer failure without closing the dialog',async()=>{
    invokeMock.mockRejectedValueOnce('invalid_signature');
    render(<StartupUpdateNotice release={release} installKind="installed"
      running={false} onDismiss={vi.fn()}/>);
    fireEvent.click(screen.getByRole('button',{name:'Install verified update'}));
    expect(await screen.findByRole('alert')).toHaveTextContent('invalid_signature');
    expect(screen.getByRole('dialog')).toBeInTheDocument();
  });
});
