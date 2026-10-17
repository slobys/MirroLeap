import {fireEvent,render,screen,waitFor} from '@testing-library/react';
import {beforeEach,describe,expect,it,vi} from 'vitest';
import AppUpdateChecker from '../AppUpdateChecker';
import type {AppUpdateInfo} from '../../utils/appUpdate';

const {invokeMock,listenMock}=vi.hoisted(()=>({
  invokeMock:vi.fn(),listenMock:vi.fn().mockResolvedValue(()=>{}),
}));
vi.mock('@tauri-apps/api/core',()=>({invoke:invokeMock}));
vi.mock('@tauri-apps/api/event',()=>({listen:listenMock}));

const release:AppUpdateInfo={
  currentVersion:'5.0.5',latestVersion:'5.0.7',status:'update_available',
  prerelease:true,
  releaseUrl:'https://github.com/slobys/MirroLeap/releases/tag/v5.0.7',
  publishedAt:'2026-10-17T09:00:00Z',
  changelog:'Bug fixes and features',
  downloads:[
    {kind:'portable',name:'MirroLeap_5.0.7_x64_Portable.zip',
      url:'https://github.com/slobys/MirroLeap/releases/download/v5.0.7/MirroLeap_5.0.7_x64_Portable.zip',
      size:6000000,sha256:'a'.repeat(64)},
    {kind:'exe',name:'MirroLeap_5.0.7_x64-setup.exe',
      url:'https://evil.invalid/malicious.exe',size:1000000,sha256:null},
  ],
};

describe('One-button MirroLeap official signed updater',()=>{
  beforeEach(()=>{invokeMock.mockReset();listenMock.mockReset().mockResolvedValue(()=>{});});
  it('does not contact or install anything until the user clicks',()=>{
    render(<AppUpdateChecker version="5.0.5"/>);
    expect(screen.getByText(/Installed version/)).toHaveTextContent('5.0.5');
    expect(screen.getByRole('button',{name:'Check for MirroLeap software updates'})).toBeEnabled();
    expect(screen.queryByRole('button',{name:'Check MirroLeap preview updates'})).not.toBeInTheDocument();
    expect(invokeMock).not.toHaveBeenCalled();
  });
  it('checks newest prerelease in the only channel and trusts canonical official assets',async()=>{
    invokeMock.mockImplementation(async(action:string)=>action==='get_app_install_kind'?'portable':
      {...release,changelog:'<script>alert(1)</script>\nNew controls'});
    const {container}=render(<AppUpdateChecker version="5.0.5"/>);
    fireEvent.click(screen.getByRole('button',{name:'Check for MirroLeap software updates'}));
    expect(await screen.findByText(/New version/)).toHaveTextContent('v5.0.7 · Preview');
    expect(invokeMock).toHaveBeenCalledWith('check_mirroleap_update');
    expect(screen.getByRole('link',{name:'Portable ZIP'})).toHaveAttribute('href',release.downloads[0].url);
    expect(screen.queryByRole('link',{name:'EXE'})).not.toBeInTheDocument();
    expect(screen.getByRole('link',{name:'View this release'})).toHaveAttribute('href',release.releaseUrl);
    expect(container.querySelector('script')).toBeNull();
    expect(screen.queryByRole('button',{name:'Download, verify and install MirroLeap update'}))
      .not.toBeInTheDocument();
  });
  it('never auto-installs in MSI or portable copies',async()=>{
    invokeMock.mockImplementation(async(action:string)=>action==='get_app_install_kind'?'other':release);
    render(<AppUpdateChecker version="5.0.5"/>);
    fireEvent.click(screen.getByRole('button',{name:'Check for MirroLeap software updates'}));
    expect(await screen.findByText(/New version/)).toBeInTheDocument();
    expect(screen.getByText(/No eligible NSIS installation/)).toBeInTheDocument();
    expect(invokeMock).not.toHaveBeenCalledWith('install_signed_update',expect.anything());
  });
  it('only installs after explicit click on detected NSIS, passing expected official version to Rust',async()=>{
    invokeMock.mockImplementation(async(action:string)=>action==='get_app_install_kind'?'installed':
      action==='install_signed_update'?undefined:release);
    render(<AppUpdateChecker version="5.0.5"/>);
    expect(invokeMock).not.toHaveBeenCalled();
    fireEvent.click(screen.getByRole('button',{name:'Check for MirroLeap software updates'}));
    const install=await screen.findByRole('button',{name:'Download, verify and install MirroLeap update'});
    expect(invokeMock).not.toHaveBeenCalledWith('install_signed_update',expect.anything());
    fireEvent.click(install);
    await waitFor(()=>expect(invokeMock).toHaveBeenCalledWith('install_signed_update',{version:'5.0.7'}));
    expect(await screen.findByText(/signed installer started/i)).toBeInTheDocument();
  });
  it('disables updates while a mirror is running',async()=>{
    invokeMock.mockImplementation(async(action:string)=>action==='get_app_install_kind'?'installed':release);
    render(<AppUpdateChecker version="5.0.5" running/>);
    fireEvent.click(screen.getByRole('button',{name:'Check for MirroLeap software updates'}));
    expect(await screen.findByRole('button',{name:'Download, verify and install MirroLeap update'})).toBeDisabled();
    expect(screen.getByText(/Stop the active phone mirror before installing/)).toBeInTheDocument();
  });
  it('does not offer downgrade or reinstall of current build',async()=>{
    invokeMock.mockResolvedValue({...release,latestVersion:'5.0.4',
      releaseUrl:'https://github.com/slobys/MirroLeap/releases/tag/v5.0.4',status:'ahead'});
    render(<AppUpdateChecker version="5.0.5"/>);
    fireEvent.click(screen.getByRole('button',{name:'Check for MirroLeap software updates'}));
    expect(await screen.findByText(/Installed build is newer/)).toBeInTheDocument();
    expect(screen.queryByRole('button',{name:'Download, verify and install MirroLeap update'}))
      .not.toBeInTheDocument();
  });
  it('shows actionable GitHub rate-limit errors without duplicate check',async()=>{
    let reject!:(reason:string)=>void;
    invokeMock.mockReturnValueOnce(new Promise((_,rejectPromise)=>{reject=rejectPromise;}));
    render(<AppUpdateChecker version="5.0.5"/>);
    const check=screen.getByRole('button',{name:'Check for MirroLeap software updates'});
    fireEvent.click(check);
    expect(check).toBeDisabled();
    fireEvent.click(check);
    expect(invokeMock).toHaveBeenCalledTimes(1);
    reject('rate_limited');
    expect(await screen.findByRole('alert')).toHaveTextContent('GitHub rate limit');
    await waitFor(()=>expect(check).toBeEnabled());
  });
  it('rejects foreign GitHub release paths',async()=>{
    invokeMock.mockResolvedValue({...release,releaseUrl:'https://evil.invalid/releases/v5.0.7'});
    render(<AppUpdateChecker version="5.0.5"/>);
    fireEvent.click(screen.getByRole('button',{name:'Check for MirroLeap software updates'}));
    expect(await screen.findByRole('alert')).toHaveTextContent('Could not verify');
  });
});
