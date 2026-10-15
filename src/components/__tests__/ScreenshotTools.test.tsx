import { act, fireEvent, render, screen, waitFor } from '@testing-library/react';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import ScreenshotTools, {screenshotErrorMessage} from '../ScreenshotTools';

const { invokeMock } = vi.hoisted(() => ({ invokeMock: vi.fn() }));
vi.mock('@tauri-apps/api/core', () => ({ invoke: invokeMock }));

describe('MirroCapture local screenshot', () => {
  beforeEach(() => invokeMock.mockReset());

  it('does not expose device identifiers to unrelated windows and requires a selected phone', () => {
    render(<ScreenshotTools device=""/>);
    expect(screen.getByRole('button', {name:'Save phone screenshot'})).toBeDisabled();
    expect(screen.getByText(/Connect and select a device/)).toBeInTheDocument();
    expect(invokeMock).not.toHaveBeenCalled();
  });

  it('triggers one explicit main-window native screenshot and reports saved file', async () => {
    invokeMock.mockResolvedValue('C:\\Users\\Alice\\Downloads\\MirroLeap-Screenshot-20261015-121235.png');
    const customPath = String.raw`D:\scrcpy-bin`;
    render(<ScreenshotTools device="emulator-5554" customPath={customPath}/>);
    const button = screen.getByRole('button', {name:'Save phone screenshot'});
    fireEvent.click(button);
    await waitFor(() => expect(invokeMock).toHaveBeenCalledWith('save_device_screenshot', {
      device:'emulator-5554', customPath,
    }));
    expect(await screen.findByRole('status')).toHaveTextContent('MirroLeap-Screenshot-20261015-121235.png');
    expect(button).toBeEnabled();
    expect(screen.getByText(/Pen annotations are not included yet/)).toBeInTheDocument();
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
});
