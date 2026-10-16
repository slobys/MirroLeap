import {fireEvent,render,screen,waitFor} from '@testing-library/react';
import {beforeEach,describe,expect,it,vi} from 'vitest';
import MirroPresentPanel from '../MirroPresentPanel';

const {invokeMock,listenMock} = vi.hoisted(()=>({
  invokeMock:vi.fn(),listenMock:vi.fn(),
}));
vi.mock('@tauri-apps/api/core',()=>({invoke:invokeMock}));
vi.mock('@tauri-apps/api/event',()=>({listen:listenMock}));

describe('MirroPresent opt-in tools in settings',()=>{
  beforeEach(()=>{
    invokeMock.mockReset();
    listenMock.mockReset().mockResolvedValue(()=>{});
    invokeMock.mockImplementation(async(command)=>{
      if (command==='presentation_get_mode') return 'off';
      if (command==='presentation_set_mode') return 'spotlight';
      return undefined;
    });
  });

  it('requires active phone mirroring, does not auto-enable spotlight or touch capture',()=>{
    render(<MirroPresentPanel device="" running={false} onRecenter={vi.fn()}/>);
    expect(screen.getByRole('button',{name:'Enable spotlight'})).toBeDisabled();
    expect(screen.getByRole('button',{name:'Refresh magnifier snapshot'})).toBeDisabled();
    expect(screen.getByText(/Start a Windows phone mirror/)).toBeInTheDocument();
    expect(invokeMock).not.toHaveBeenCalled();
  });

  it('starts presentation only on click and restores touch when exiting',async()=>{
    const onRecenter=vi.fn().mockResolvedValue(undefined);
    invokeMock.mockImplementation(async(command,args)=>{
      if (command==='presentation_get_mode') return 'off';
      if (command==='presentation_set_mode') return args.mode;
      return undefined;
    });
    render(<MirroPresentPanel device="emulator-5554" running={true} onRecenter={onRecenter}/>);
    await waitFor(()=>expect(invokeMock).toHaveBeenCalledWith('presentation_get_mode',{device:'emulator-5554'}));
    expect(invokeMock).toHaveBeenCalledTimes(1);
    fireEvent.click(screen.getByRole('button',{name:'Enable spotlight'}));
    await waitFor(()=>expect(screen.getByRole('button',{name:'Enable spotlight'})).toHaveAttribute('aria-pressed','true'));
    expect(invokeMock).toHaveBeenCalledWith('presentation_set_mode',{device:'emulator-5554',mode:'spotlight'});
    fireEvent.click(screen.getByRole('button',{name:'Exit presentation and restore touch'}));
    await waitFor(()=>expect(screen.getByRole('button',{name:'Exit presentation and restore touch'})).toBeDisabled());
    expect(invokeMock).toHaveBeenCalledWith('presentation_set_mode',{device:'emulator-5554',mode:'off'});
    fireEvent.click(screen.getByRole('button',{name:'Center presentation window'}));
    await waitFor(()=>expect(onRecenter).toHaveBeenCalledWith('emulator-5554'));
  });

  it('does not silently capture a screenshot; magnifier refresh is explicit',async()=>{
    invokeMock.mockImplementation(async(command,args)=>{
      if (command==='presentation_get_mode') return 'off';
      if (command==='presentation_set_mode') return args.mode;
      if (command==='presentation_refresh_snapshot') return undefined;
      throw Error('unexpected IPC');
    });
    render(<MirroPresentPanel device="emulator-5554" running={true}
      customPath="D:/adb" onRecenter={vi.fn()}/>);
    fireEvent.click(screen.getByRole('button',{name:'Enable snapshot magnifier'}));
    await waitFor(()=>expect(screen.getByRole('button',{name:'Refresh magnifier snapshot'})).toBeEnabled());
    expect(invokeMock).not.toHaveBeenCalledWith('presentation_refresh_snapshot',expect.anything());
    expect(screen.getByText(/No still frame loaded/)).toBeInTheDocument();
    fireEvent.click(screen.getByRole('button',{name:'Refresh magnifier snapshot'}));
    await waitFor(()=>expect(invokeMock).toHaveBeenCalledWith('presentation_refresh_snapshot',{
      device:'emulator-5554',customPath:'D:/adb'
    }));
    await waitFor(()=>expect(screen.getByText(/Still image loaded/)).toBeInTheDocument());
    expect(screen.getByText(/This is not live video/)).toBeInTheDocument();
  });

  it('reports native failure instead of visually pretending the tool activated',async()=>{
    invokeMock.mockImplementation(async(command)=>{
      if (command==='presentation_get_mode') return 'off';
      throw Error('WebView2 frame is unavailable');
    });
    render(<MirroPresentPanel device="emulator-5554" running={true} onRecenter={vi.fn()}/>);
    fireEvent.click(screen.getByRole('button',{name:'Enable spotlight'}));
    expect(await screen.findByRole('alert')).toHaveTextContent('WebView2 frame is unavailable');
    expect(screen.getByRole('button',{name:'Enable spotlight'})).toHaveAttribute('aria-pressed','false');
  });
});
