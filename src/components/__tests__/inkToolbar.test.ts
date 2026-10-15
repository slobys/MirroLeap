import { readFileSync } from 'node:fs';
import { resolve } from 'node:path';
// @ts-ignore: jsdom is available at runtime through Vitest; this project has no @types/jsdom.
import { JSDOM } from 'jsdom';
import { describe, expect, it, vi } from 'vitest';

const html = readFileSync(resolve(process.cwd(), 'public/phone-controls.html'), 'utf8');
const settle = () => new Promise<void>(resolve => setTimeout(resolve, 0));

function mount(invoke: (command: string, args: { action: string }) => Promise<boolean>) {
  const alert = vi.fn();
  const dom = new JSDOM(html, {
    url: 'https://localhost/phone-controls.html',
    runScripts: 'dangerously',
    beforeParse(window: any) {
      Object.defineProperty(window, '__TAURI__', { value: { core: { invoke } } });
      window.alert = alert;
    },
  });
  return { dom, alert, body: dom.window.document.body };
}

describe('native-acknowledged annotation hover toolbar', () => {
  it('uses native enabled state rather than optimistic local toggling', async () => {
    let enabled = false;
    const invoke = vi.fn(async (_: string, { action }: { action: string }) => {
      if (action === 'ink_status') return enabled;
      if (action === 'ink_toggle') { enabled = !enabled; return enabled; }
      return enabled;
    });
    const { dom, body } = mount(invoke);
    await settle();

    const button = dom.window.document.getElementById('ink-toggle') as HTMLButtonElement;
    expect(body.classList.contains('inking')).toBe(false);
    button.click();
    await settle();
    expect(button.getAttribute('aria-pressed')).toBe('true');
    expect(body.classList.contains('inking')).toBe(true);
    expect(button.disabled).toBe(false);
    button.click();
    await settle();
    expect(body.classList.contains('inking')).toBe(false);
    expect(button.getAttribute('aria-pressed')).toBe('false');
    dom.window.close();
  });

  it('ignores double clicks while WebView2 is acknowledging the first command', async () => {
    let release!: (result: boolean) => void;
    let enabled = false;
    const invoke = vi.fn((_cmd: string, { action }: { action: string }) => {
      if (action === 'ink_toggle') {
        return new Promise<boolean>(resolve => {
          release = value => { enabled = value; resolve(value); };
        });
      }
      return Promise.resolve(enabled);
    });
    const { dom, body } = mount(invoke);
    await settle();
    const button = dom.window.document.getElementById('ink-toggle') as HTMLButtonElement;
    button.click();
    button.click();
    expect(invoke.mock.calls.filter(c => c[1].action === 'ink_toggle')).toHaveLength(1);
    expect(button.disabled).toBe(true);
    release(true);
    await settle();
    expect(button.disabled).toBe(false);
    expect(body.classList.contains('inking')).toBe(true);
    dom.window.close();
  });

  it('does not indicate drawing success when native page is unready', async () => {
    const invoke = vi.fn(async (_cmd: string, { action }: { action: string }) => {
      if (action === 'ink_toggle') throw new Error('画布尚未加载完毕');
      return false;
    });
    const { dom, body, alert } = mount(invoke);
    await settle();
    const button = dom.window.document.getElementById('ink-toggle') as HTMLButtonElement;
    button.click();
    await settle();
    expect(body.classList.contains('inking')).toBe(false);
    expect(button.getAttribute('aria-pressed')).toBe('false');
    expect(button.disabled).toBe(false);
    expect(alert).toHaveBeenCalledTimes(1);
    expect(dom.window.document.getElementById('status')?.getAttribute('data-error')).toBe('true');
    dom.window.close();
  });

  it('resynchronizes pen state after native overlay reload or focus change', async () => {
    let enabled = true;
    const invoke = vi.fn(async (_cmd: string, { action }: { action: string }) => {
      if (action === 'ink_status') return enabled;
      return enabled;
    });
    const { dom, body } = mount(invoke);
    await settle();
    expect(body.classList.contains('inking')).toBe(true);
    enabled = false;
    dom.window.dispatchEvent(new dom.window.Event('focus'));
    await settle();
    expect(body.classList.contains('inking')).toBe(false);
    dom.window.close();
  });
});
