import { fireEvent, render, screen } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import ControlPanel from '.';
import { RenderDriverSupport, ScrcpyConfig } from '../../hooks/useScrcpy';

const supportedRenderers: RenderDriverSupport = {
    hostOs: 'windows',
    supportsRenderDriver: true,
    supportedDrivers: [
        { id: 'direct3d', label: 'D3D11 (Direct3D)' },
        { id: 'opengl', label: 'OpenGL' }
    ]
};

const unsupportedRenderers: RenderDriverSupport = {
    hostOs: 'windows',
    supportsRenderDriver: false,
    supportedDrivers: []
};

function createConfig(overrides: Partial<ScrcpyConfig> = {}): ScrcpyConfig {
    return {
        device: 'device-1',
        sessionMode: 'mirror',
        fps: 60,
        res: '0',
        ...overrides
    };
}

function renderControlPanel(config: ScrcpyConfig, support: RenderDriverSupport, setConfig = vi.fn()) {
    render(
        <ControlPanel
            config={config}
            setConfig={setConfig}
            onStart={vi.fn()}
            onStop={vi.fn()}
            isRunning={false}
            onListOptions={vi.fn()}
            detectedCameras={[]}
            renderDriverSupport={support}
        />
    );

    return setConfig;
}

async function openRendererSelect(user: ReturnType<typeof userEvent.setup>) {
    const advanced = screen.queryByText('Advanced encoding & display');
    if (advanced) await user.click(advanced);
    const label = screen.getByText('Graphics Renderer');
    const container = label.parentElement as HTMLElement;
    const button = container.querySelector('button') as HTMLButtonElement;
    await user.click(button);
}

describe('simplified HID settings', () => {
    it('keeps the keyboard and bitrate while hiding mouse and no-video HID options', () => {
        renderControlPanel(createConfig({
            hidKeyboard:true, hidMouse:true, otgPure:true
        }), supportedRenderers);
        expect(screen.getByText('HID Keyboard')).toBeInTheDocument();
        expect(screen.queryByText(/HID Mouse/i)).not.toBeInTheDocument();
        expect(screen.queryByText(/Pure HID/i)).not.toBeInTheDocument();
        expect(screen.getByText('Bitrate')).toBeInTheDocument();
    });
});

describe('Android phone screen recording mode boundaries', () => {
    it('clears device screen recording when switching to camera or virtual desktop', async () => {
        const user = userEvent.setup();
        const setConfig = vi.fn();
        renderControlPanel(createConfig({record:true}), supportedRenderers, setConfig);
        await user.click(screen.getByText('Camera'));
        expect(setConfig).toHaveBeenCalledWith(expect.objectContaining({
            sessionMode:'camera',record:false
        }));
        await user.click(screen.getByText('Desktop'));
        expect(setConfig).toHaveBeenLastCalledWith(expect.objectContaining({
            sessionMode:'desktop',record:false
        }));
    });
});

describe('single resolution control with dynamic video parameter summary', () => {
    it('removes the duplicate scene preset while preserving the current custom encoding details', () => {
        renderControlPanel(createConfig({
            res:'2560',bitrate:16,codec:'h265',fps:30,audioEnabled:false,
            renderDriver:'opengl',rotation:'90'
        }), supportedRenderers);
        expect(screen.queryByRole('combobox',{name:/Profiles|Scenario profile/})).not.toBeInTheDocument();
        expect(screen.queryByText(/场景预设/)).not.toBeInTheDocument();
        expect(screen.getAllByText('Resolution')).toHaveLength(1);
        expect(screen.queryByText('Original')).not.toBeInTheDocument();
        expect(screen.getByTestId('settings-video-summary')).toHaveTextContent(
            'Current settings: 2K · 16Mbps · up to 30 fps · H265'
        );
        expect(screen.getByText(/Use 2K when it runs smoothly/)).toBeInTheDocument();
    });

    it('refreshes the parameter explanation whenever the saved resolution/bitrate/FPS changes', () => {
        const common = {setConfig:vi.fn(),onStart:vi.fn(),onStop:vi.fn(),
            isRunning:false,onListOptions:vi.fn(),detectedCameras:[],
            renderDriverSupport:supportedRenderers};
        const {rerender}=render(<ControlPanel {...common}
            config={createConfig({res:'800',bitrate:3,fps:60,codec:'h264'})}/>);
        expect(screen.getByTestId('settings-video-summary')).toHaveTextContent(
            'Current settings: 480p · 3Mbps · up to 60 fps · H264'
        );
        rerender(<ControlPanel {...common}
            config={createConfig({res:'3840',bitrate:20,fps:30,codec:'h264'})}/>);
        expect(screen.getByTestId('settings-video-summary')).toHaveTextContent(
            'Current settings: 4K · 20Mbps · up to 30 fps · H264'
        );
    });

    it('keeps advanced codec, rotation and renderer available but collapsed by default', async () => {
        const user = userEvent.setup();
        renderControlPanel(createConfig({res:'1920',bitrate:8,codec:'h264'}), supportedRenderers);
        const advanced = screen.getByText('Advanced encoding & display').closest('details') as HTMLDetailsElement;
        expect(advanced).not.toHaveAttribute('open');
        expect(advanced).toContainElement(screen.getByText('Graphics Renderer'));
        await user.click(screen.getByText('Advanced encoding & display'));
        expect(advanced).toHaveAttribute('open');
        expect(screen.getByText('Resolution')).toBeInTheDocument();
        expect(screen.getByText('Bitrate')).toBeInTheDocument();
    });

    it('keeps advanced settings open when a manual codec or renderer value changes', async () => {
        const user = userEvent.setup();
        const settings = createConfig({res:'1920',bitrate:8,codec:'h264'});
        const common = {setConfig:vi.fn(),onStart:vi.fn(),onStop:vi.fn(),
            isRunning:false,onListOptions:vi.fn(),detectedCameras:[],
            renderDriverSupport:supportedRenderers};
        const {rerender}=render(<ControlPanel {...common} config={settings}/>);
        await user.click(screen.getByText('Advanced encoding & display'));
        const advanced=screen.getByText('Advanced encoding & display').closest('details') as HTMLDetailsElement;
        expect(advanced).toHaveAttribute('open');
        rerender(<ControlPanel {...common} config={{...settings,codec:'h265'}}/>);
        expect(screen.getByText('Advanced encoding & display').closest('details')).toHaveAttribute('open');
    });

    it('commits manual bitrate slider changes made with the keyboard', () => {
        const setConfig=renderControlPanel(
            createConfig({res:'1920',bitrate:8,fps:60,codec:'h264'}), supportedRenderers);
        const slider=screen.getByRole('slider',{name:'Bitrate'});
        fireEvent.change(slider,{target:{value:'11'}});
        fireEvent.keyUp(slider,{key:'ArrowRight',target:{value:'11'}});
        expect(setConfig).toHaveBeenCalledWith(expect.objectContaining({bitrate:11}));
    });

    it('preserves the legacy manually selected codec and automatic FPS while explaining 2K use', () => {
        renderControlPanel(createConfig({res:'2560',bitrate:16,codec:'h265',fps:undefined}), supportedRenderers);
        expect(screen.getByTestId('settings-video-summary')).toHaveTextContent(
            '2K · 16Mbps · automatic fps · H265'
        );
        expect(screen.getByText(/Use 2K when it runs smoothly/)).toBeInTheDocument();
    });
});

describe('one-click mirror resolution selector', () => {
    it('requests automatic stream restart when choosing a new size while mirroring', async () => {
        const user = userEvent.setup();
        const onResolutionChange = vi.fn();
        render(
            <ControlPanel config={createConfig({ res: '1920' })}
                setConfig={vi.fn()} onStart={vi.fn()} onStop={vi.fn()}
                onResolutionChange={onResolutionChange} isSwitchingResolution={false}
                isRunning={true} onListOptions={vi.fn()} detectedCameras={[]}
                renderDriverSupport={supportedRenderers} />
        );
        const label = screen.getByText('Resolution');
        await user.click(label.parentElement?.querySelector('button') as HTMLButtonElement);
        await user.click(screen.getByText('720p'));
        expect(onResolutionChange).toHaveBeenCalledExactlyOnceWith('1280');
        expect(screen.getByRole('status')).toHaveTextContent('restarts briefly');
    });

    it('disables selector while the previous stream is applying', () => {
        render(<ControlPanel config={createConfig()} setConfig={vi.fn()}
            onStart={vi.fn()} onStop={vi.fn()} isRunning={true}
            isSwitchingResolution={true} onResolutionChange={vi.fn()}
            onListOptions={vi.fn()} detectedCameras={[]}
            renderDriverSupport={supportedRenderers} />);
        const label = screen.getByText('Resolution');
        expect(label.parentElement?.querySelector('button')).toBeDisabled();
        expect(screen.getByRole('status')).toHaveTextContent('Applying resolution');
    });

    it('locks native session Stop while automatic video switching is underway', () => {
        render(<ControlPanel config={createConfig({res:'2560',fps:60,bitrate:16,codec:'h264'})}
            setConfig={vi.fn()} onStart={vi.fn()} onStop={vi.fn()} isRunning
            isSwitchingResolution onListOptions={vi.fn()}
            detectedCameras={[]} renderDriverSupport={supportedRenderers}/>);
        expect(screen.getByText('Applying video settings…').closest('button')).toBeDisabled();
    });

    it('warns that recording must stop before switching video resolution', () => {
        render(<ControlPanel config={createConfig({ record: true })}
            setConfig={vi.fn()} onStart={vi.fn()} onStop={vi.fn()}
            onResolutionChange={vi.fn()} isRunning={true}
            onListOptions={vi.fn()} detectedCameras={[]}
            renderDriverSupport={supportedRenderers} />);
        expect(screen.getByRole('status')).toHaveTextContent('Stop recording');
    });
});

describe('ControlPanel renderer selector', () => {
    describe('with supported renderers and default config', () => {
        let user: ReturnType<typeof userEvent.setup>;

        beforeEach(() => {
            user = userEvent.setup();
            renderControlPanel(createConfig(), supportedRenderers);
        });

        it('shows graphics renderer label', () => {
            expect(screen.getByText('Graphics Renderer')).toBeInTheDocument();
        });

        it('shows auto renderer value by default', () => {
            expect(screen.getAllByText('Auto').length).toBeGreaterThan(0);
        });

        it('shows supported renderer option', async () => {
            await openRendererSelect(user);
            expect(screen.getByText('OpenGL')).toBeInTheDocument();
        });

        it('hides unsupported renderer option', async () => {
            await openRendererSelect(user);
            expect(screen.queryByText('Metal')).not.toBeInTheDocument();
        });
    });

    describe('when selecting renderers', () => {
        it('updates renderDriver when selecting supported renderer', async () => {
            const user = userEvent.setup();
            const setConfig = renderControlPanel(createConfig(), supportedRenderers);
            await openRendererSelect(user);
            await user.click(screen.getByText('D3D11 (Direct3D)'));
            expect(setConfig).toHaveBeenCalledWith(expect.objectContaining({ renderDriver: 'direct3d' }));
        });

        it('clears renderDriver when selecting auto renderer', async () => {
            const user = userEvent.setup();
            const setConfig = renderControlPanel(createConfig({ renderDriver: 'direct3d' }), supportedRenderers);
            await openRendererSelect(user);
            await user.click(screen.getByText('Auto'));
            expect(setConfig).toHaveBeenCalledWith(expect.objectContaining({ renderDriver: undefined }));
        });
    });

    describe('with unsupported renderer capability', () => {
        it('keeps renderer options auto only when support is unavailable', async () => {
            const user = userEvent.setup();
            renderControlPanel(createConfig(), unsupportedRenderers);
            await openRendererSelect(user);
            expect(screen.queryByText('OpenGL')).not.toBeInTheDocument();
        });
    });

    describe('across session modes', () => {
        it('shows renderer selector in camera mode', () => {
            renderControlPanel(createConfig({ sessionMode: 'camera' }), supportedRenderers);
            expect(screen.getByText('Graphics Renderer')).toBeInTheDocument();
        });

        it('shows renderer selector in desktop mode', () => {
            renderControlPanel(createConfig({ sessionMode: 'desktop' }), supportedRenderers);
            expect(screen.getByText('Graphics Renderer')).toBeInTheDocument();
        });
    });
});
