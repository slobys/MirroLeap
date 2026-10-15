import { readFileSync } from 'node:fs';
import { join } from 'node:path';
import { JSDOM } from 'jsdom';
import { describe, expect, it } from 'vitest';

const html = readFileSync(join(process.cwd(), 'public/phone-controls.html'), 'utf8');
const nextTask = () => new Promise(resolve => setTimeout(resolve, 0));

// Exercise the same self-contained HTML that Tauri loads inside its WebView2
// child HWND; stub only its narrow native IPC boundary.
function setup({ failToggle = false } = {}) {
  const actions = [];
  let enabled = false;
  const dom = new JSDOM(html, {
    url: 'http://localhost/phone-controls.html',
    runScripts: 'dangerously',
    pretendToBeVisual: true,
    beforeParse(window) {
      window.alert = () => {};
      window.__TAURI__ = { core: { invoke: async (command, { action }) => {
        expect(command).toBe('device_frame_control');
        actions.push(action);
        if (action === 'ink_toggle') {
          if (failToggle) throw new Error('native canvas not ready');
          enabled = !enabled;
        }
        if (action === 'ink_reset') enabled = false;
        return enabled;
      } } };
    },
  });
  return { dom, document: dom.window.document, actions };
}

describe('real Tauri phone-controls HTML', () => {
  it('keeps primary window actions adjacent and has a non-scrolling palette', async () => {
    const { dom, document, actions } = setup();
    const ids = [...document.querySelectorAll('.session-actions button')].map(b => b.id);
    expect(ids).toEqual(['drag', 'minimize', 'close']);
    expect([...document.querySelectorAll('.annotation-actions button')].map(b => b.id))
      .toEqual(['ink-toggle', 'restore']);
    expect(dom.window.getComputedStyle(document.querySelector('#ink-tools')).display).toBe('none');
    document.getElementById('ink-toggle').click();
    await nextTask();
    await nextTask();
    expect(actions).toContain('ink_toggle');
    expect(document.body.classList.contains('inking')).toBe(true);
    expect(document.querySelectorAll('.palette-actions button')).toHaveLength(4);
    expect(document.querySelectorAll('.swatches button')).toHaveLength(4);
    expect(document.querySelectorAll('.sizes button')).toHaveLength(3);
    expect(dom.window.getComputedStyle(document.querySelector('#ink-tools')).overflow).toBe('hidden');
    for (const layout of ['side-right', 'side-left', 'stack-left', 'stack-right']) {
      dom.window.scrcpyControls.setLayout(layout);
      expect(document.body.dataset.layout).toBe(layout);
      expect(dom.window.getComputedStyle(document.querySelector('#ink-tools')).display).toBe('flex');
    }
    dom.window.scrcpyControls.setLayout('invalid-layout');
    expect(document.body.dataset.layout).toBe('stack-right');
    dom.window.close();
  });

  it('uses distinct eraser, undo, clear and direct width actions without closing ink', async () => {
    const { dom, document, actions } = setup();
    document.getElementById('ink-toggle').click();
    await nextTask(); await nextTask();
    document.getElementById('eraser').click();
    await nextTask();
    expect(document.getElementById('eraser').getAttribute('aria-pressed')).toBe('true');
    expect(document.getElementById('eraser').querySelector('svg')).not.toBeNull();
    document.getElementById('size-9').click();
    document.getElementById('red').click();
    document.getElementById('undo').click();
    document.getElementById('clear').click();
    await nextTask(); await nextTask();
    expect(actions).toEqual(expect.arrayContaining([
      'ink_erase', 'ink_size_9', 'ink_color_red', 'ink_undo', 'ink_clear',
    ]));
    expect(document.body.classList.contains('inking')).toBe(true);
    expect(document.getElementById('size-9').getAttribute('aria-pressed')).toBe('true');
    expect(document.getElementById('pen').getAttribute('aria-pressed')).toBe('true');
    dom.window.close();
  });

  it('one-click restore clears drawing AND returns Android input', async () => {
    const { dom, document, actions } = setup();
    document.getElementById('ink-toggle').click();
    await nextTask(); await nextTask();
    document.getElementById('restore').click();
    await nextTask();
    expect(actions).toContain('ink_reset');
    expect(document.body.classList.contains('inking')).toBe(false);
    expect(document.getElementById('ink-toggle').getAttribute('aria-pressed')).toBe('false');
    dom.window.close();
  });

  it('does not reveal the palette when native ink activation fails', async () => {
    const { dom, document } = setup({ failToggle: true });
    document.getElementById('ink-toggle').click();
    await nextTask(); await nextTask();
    expect(document.body.classList.contains('inking')).toBe(false);
    expect(document.getElementById('status').dataset.error).toBe('true');
    expect(document.getElementById('ink-toggle').disabled).toBe(false);
    dom.window.close();
  });
});
