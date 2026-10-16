import { act, fireEvent, render, screen, waitFor } from '@testing-library/react';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import ScreenshotTools, {pngDataUrlBytes, screenshotErrorMessage} from '../ScreenshotTools';

const { invokeMock, imageMock, writeImageMock, closeMock } = vi.hoisted(() => ({
  invokeMock: vi.fn(),
  imageMock: vi.fn(),
  writeImageMock: vi.fn(),
  closeMock: vi.fn(),
}));
vi.mock('@tauri-apps/api/core', () => ({ invoke: invokeMock }));
vi.mock('@tauri-apps/api/image', () => ({ Image: { fromBytes: imageMock } }));
vi.mock('@tauri-apps/plugin-clipboard-manager', () => ({ writeImage: writeImageMock }));
const tinyPng = 'data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mP8/x8AAwMCAO+a6PoAAAAASUVORK5CYII=';

describe('MirroCapture local screenshot', () => {
  beforeEach(() => {
    invokeMock.mockReset();
    imageMock.mockReset();
    closeMock.mockReset();
    writeImageMock.mockReset();
    closeMock.mockResolvedValue(undefined);
    imageMock.mockResolvedValue({ close: closeMock });
  });

  it('does not expose device identifiers to unrelated windows and requires a selected phone', () => {
    render(<ScreenshotTools device=""/>);
    expect(screen.getByRole('button', {name:'Save phone screenshot'})).toBeDisabled();
    expect(screen.getByText(/Connect and select a device/)).toBeInTheDocument();
    expect(invokeMock).not.toHaveBeenCalled();
  });

  it('triggers one explicit main-window native screenshot and reports saved file', async () => {
    invokeMock.mockImplementation(async (cmd) => cmd === 'preview_saved_screenshot'
      ? tinyPng : 'C:\\Users\\Alice\\Downloads\\MirroLeap-Screenshot-20261015-121235.png');
    const customPath = String.raw`D:\scrcpy-bin`;
    render(<ScreenshotTools device="emulator-5554" customPath={customPath}/>);
    const button = screen.getByRole('button', {name:'Save phone screenshot'});
    fireEvent.click(button);
    await waitFor(() => expect(invokeMock).toHaveBeenCalledWith('save_device_screenshot', {
      device:'emulator-5554', customPath,
    }));
    expect(await screen.findByRole('status')).toHaveTextContent('MirroLeap-Screenshot-20261015-121235.png');
    expect(await screen.findByAltText('Preview of the most recently saved phone screenshot')).toBeInTheDocument();
    expect(invokeMock).toHaveBeenCalledWith('preview_saved_screenshot', {
      path:'C:\\Users\\Alice\\Downloads\\MirroLeap-Screenshot-20261015-121235.png',
    });
    expect(button).toBeEnabled();
    expect(screen.getByText(/Annotated capture requires an active mirrored phone/)).toBeInTheDocument();
  });

  it('prevents concurrent screenshot capture calls while the first one is pending', async () => {
    let resolveCapture!: (path: string) => void;
    invokeMock.mockReturnValue(new Promise((resolve) => { resolveCapture = resolve; }));
    render(<ScreenshotTools device="192.168.2.226:5555"/>);
    const button = screen.getByRole('button', {name:'Save phone screenshot'});
    fireEvent.click(button);
    expect(button).toBeDisabled();
    fireEvent.click(button);
    expect(invokeMock).toHaveBeenCalledTimes(1);
    await act(async () => resolveCapture('C:\\Downloads\\shot.png'));
    expect(await screen.findByRole('status')).toHaveTextContent('shot.png');
    expect(button).toBeEnabled();
  });

  it('normalizes native ADB failure messages for localized feedback', () => {
    expect(screenshotErrorMessage('Android authorization required'))
      .toBe('Android authorization required');
    expect(screenshotErrorMessage(new Error('Disconnected while saving')))
      .toBe('Disconnected while saving');
  });

  it('saves annotated screenshots only on explicit click, retaining existing pen state', async () => {
    invokeMock.mockImplementation(async (cmd) => cmd === 'preview_saved_screenshot'
      ? tinyPng : 'C:\\Downloads\\MirroLeap-Annotated-123.png');
    render(<ScreenshotTools device="emulator-5554"/>);
    fireEvent.click(screen.getByRole('button', {name: 'Save annotated screenshot'}));
    expect(invokeMock).toHaveBeenCalledTimes(1);
    expect(invokeMock).toHaveBeenCalledWith('save_annotated_screenshot', {
      device: 'emulator-5554', customPath: undefined,
    });
    expect(await screen.findByRole('status')).toHaveTextContent('MirroLeap-Annotated-123.png');
    expect(await screen.findByRole('button',{name:'Copy screenshot to clipboard'})).toBeInTheDocument();
    expect(screen.getByRole('button', {name:'Save phone screenshot'})).toBeEnabled();
  });

  it('prevents two capture modes from running concurrently', async () => {
    let complete!: (path: string) => void;
    invokeMock.mockReturnValue(new Promise(resolve => { complete = resolve; }));
    render(<ScreenshotTools device="emulator-5554"/>);
    fireEvent.click(screen.getByRole('button', {name:'Save annotated screenshot'}));
    expect(screen.getByRole('button', {name:'Save phone screenshot'})).toBeDisabled();
    expect(screen.getByRole('button', {name:'Save annotated screenshot'})).toBeDisabled();
    fireEvent.click(screen.getByRole('button', {name:'Save phone screenshot'}));
    expect(invokeMock).toHaveBeenCalledTimes(1);
    await act(async () => complete('C:\\Downloads\\MirroLeap-Annotated.png'));
    expect(screen.getByRole('button', {name:'Save phone screenshot'})).toBeEnabled();
  });

  it('copies preview to clipboard only when clicked and releases its native image resource', async () => {
    invokeMock.mockImplementation(async (cmd) => cmd === 'preview_saved_screenshot'
      ? tinyPng : 'C:\\Downloads\\MirroLeap-Annotated-20261016-130000.png');
    render(<ScreenshotTools device="emulator-5554"/>);
    fireEvent.click(screen.getByRole('button',{name:'Save annotated screenshot'}));
    const copy = await screen.findByRole('button',{name:'Copy screenshot to clipboard'});
    expect(writeImageMock).not.toHaveBeenCalled();
    fireEvent.click(copy);
    await waitFor(()=>expect(writeImageMock).toHaveBeenCalledTimes(1));
    expect(imageMock).toHaveBeenCalledWith(expect.any(Uint8Array));
    expect(closeMock).toHaveBeenCalledTimes(1);
    expect(await screen.findByText(/Copied image. Paste into any desktop app/)).toBeInTheDocument();
  });

  it('keeps the saved file when preview is unavailable', async () => {
    invokeMock.mockImplementation(async (cmd) => {
      if (cmd === 'preview_saved_screenshot') throw Error('Screenshot too large to preview');
      return 'C:\\Downloads\\MirroLeap-Screenshot-20261016-130000.png';
    });
    render(<ScreenshotTools device="emulator-5554"/>);
    fireEvent.click(screen.getByRole('button',{name:'Save phone screenshot'}));
    expect(await screen.findByText(/Screenshot saved but preview unavailable/)).toHaveTextContent('too large');
    expect(screen.getByText(/MirroLeap-Screenshot-20261016-130000.png/)).toBeInTheDocument();
    expect(screen.queryByRole('button',{name:'Copy screenshot to clipboard'})).not.toBeInTheDocument();
  });

  it('rejects bad PNG previews before loading the system clipboard', () => {
    expect(Array.from(pngDataUrlBytes(tinyPng).slice(0,4))).toEqual([137,80,78,71]);
    expect(() => pngDataUrlBytes('data:text/html;base64,Zg==')).toThrow();
    expect(() => pngDataUrlBytes('data:image/png;base64,Zg==')).toThrow();
  });

  it('closes a sensitive preview without deleting the screenshot file', async () => {
    invokeMock.mockImplementation(async (cmd) => cmd === 'preview_saved_screenshot'
      ? tinyPng : 'C:\\Downloads\\MirroLeap-Screenshot-20261016-140000.png');
    render(<ScreenshotTools device="emulator-5554"/>);
    fireEvent.click(screen.getByRole('button',{name:'Save phone screenshot'}));
    const close = await screen.findByRole('button',{name:'Close screenshot preview'});
    fireEvent.click(close);
    expect(screen.queryByAltText('Preview of the most recently saved phone screenshot')).not.toBeInTheDocument();
    expect(screen.queryByRole('button',{name:'Copy screenshot to clipboard'})).not.toBeInTheDocument();
    expect(screen.getByText(/MirroLeap-Screenshot-20261016-140000.png/)).toBeInTheDocument();
    expect(writeImageMock).not.toHaveBeenCalled();
  });

  it('does not reveal an earlier phone screenshot after switching the selected device', async () => {
    let finish!: (savedPath: string) => void;
    invokeMock.mockReturnValueOnce(new Promise(resolve => {finish = resolve;}));
    const {rerender} = render(<ScreenshotTools device="emulator-5554"/>);
    fireEvent.click(screen.getByRole('button', {name:'Save phone screenshot'}));
    rerender(<ScreenshotTools device="emulator-5556"/>);
    await act(async () => finish('C:\\Downloads\\MirroLeap-Screenshot-20261016-130000.png'));
    expect(screen.queryByText(/MirroLeap-Screenshot-20261016-130000.png/)).not.toBeInTheDocument();
    expect(screen.queryByAltText('Preview of the most recently saved phone screenshot')).not.toBeInTheDocument();
    expect(invokeMock).toHaveBeenCalledTimes(1);
  });

  it('reports clipboard errors without deleting the saved PNG or preview', async () => {
    invokeMock.mockImplementation(async (cmd) => cmd === 'preview_saved_screenshot'
      ? tinyPng : 'C:\\Downloads\\MirroLeap-Screenshot-20261016-130000.png');
    writeImageMock.mockRejectedValue(new Error('Clipboard is unavailable'));
    render(<ScreenshotTools device="emulator-5554"/>);
    fireEvent.click(screen.getByRole('button',{name:'Save phone screenshot'}));
    fireEvent.click(await screen.findByRole('button',{name:'Copy screenshot to clipboard'}));
    expect(await screen.findByText(/Clipboard copy failed: Clipboard is unavailable/)).toBeInTheDocument();
    expect(closeMock).toHaveBeenCalledOnce();
    expect(screen.getByAltText('Preview of the most recently saved phone screenshot')).toBeInTheDocument();
    expect(screen.getByText(/MirroLeap-Screenshot-20261016-130000.png/)).toBeInTheDocument();
  });
});
