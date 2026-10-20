import { readFileSync } from 'node:fs';
import { join } from 'node:path';
import { JSDOM } from 'jsdom';
import { describe, expect, it } from 'vitest';

const html = readFileSync(join(process.cwd(), 'public/phone-controls.html'), 'utf8');
const nextTask = () => new Promise(resolve => setTimeout(resolve, 0));

// Exercise the same self-contained HTML that Tauri loads inside its WebView2
// child HWND; stub only its narrow native IPC boundary.
function setup({ failToggle = false, failAction = null, failReason = 'Native action unavailable' } = {}) {
  const actions = [];
  let enabled = false;
  let quick = false;
  let presenter = false;
  let presenterMode = 'off';
  let clickEffect = false;
  let top = false;
  let remember = true;
  const dom = new JSDOM(html, {
    url: 'http://localhost/phone-controls.html',
    runScripts: 'dangerously',
    pretendToBeVisual: true,
    beforeParse(window) {
      window.alert = () => {};
      window.__TAURI__ = { core: { invoke: async (command, { action }) => {
        expect(command).toBe('device_frame_control');
        actions.push(action);
        if (action === failAction) throw new Error(failReason);
        if (action === 'ink_toggle') {
          if (failToggle) throw new Error('native canvas not ready');
          enabled = !enabled;
          quick = false;
          presenter = false;
          presenterMode = 'off';
        }
        if (action === 'ink_reset') { enabled = false; quick = false; presenter = false; presenterMode='off'; }
        if (action === 'quick_toggle') {
          quick = !quick; enabled = false; presenter = false; presenterMode='off';
        }
        if (action === 'present_status') return presenter;
        if (action === 'present_is_spotlight') return presenterMode === 'spotlight';
        if (action === 'present_is_magnifier') return presenterMode === 'magnifier';
        if (action === 'present_is_live') return presenterMode === 'live';
        if (action === 'creator_click_status') return clickEffect;
        if (action === 'creator_click_toggle') return (clickEffect = !clickEffect);
        if (action === 'present_toggle') {
          presenter = !presenter;
          enabled = false;quick = false;
          if (!presenter) presenterMode = 'off';
          return presenter;
        }
        if (action === 'present_spotlight') { presenterMode = 'spotlight'; return true; }
        if (action === 'present_magnifier') { presenterMode = 'magnifier'; return true; }
        if (action === 'present_live') { presenterMode = presenterMode === 'live' ? 'off' : 'live'; return presenterMode === 'live'; }
        if (action === 'present_refresh') return true;
        if (action === 'present_off') { presenterMode = 'off'; return false; }
        if (action === 'quick_status') return quick;
        if (action === 'quick_top_status') return top;
        if (action === 'quick_top_toggle') return (top = !top);
        if (action === 'quick_remember_status') return remember;
        if (action === 'quick_remember_toggle') return (remember = !remember);
        return action.startsWith('quick_') ? true : enabled;
      } } };
    },
  });
  return { dom, document: dom.window.document, actions, setTop(value) { top = !!value; } };
}

describe('real Tauri phone-controls HTML', () => {
  it('keeps primary window actions adjacent and has a non-scrolling palette', async () => {
    const { dom, document, actions } = setup();
    const ids = [...document.querySelectorAll('.session-actions button')].map(b => b.id);
    expect(ids).toEqual(['drag', 'minimize', 'close']);
    expect([...document.querySelectorAll('.annotation-actions button')].map(b => b.id))
      .toEqual(['ink-toggle', 'quick-toggle', 'present-toggle', 'restore']);
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
    for (const layout of [
      'side-right', 'side-left', 'stack-left', 'stack-right',
      'short-right', 'short-left', 'short-ink-right', 'short-ink-left',
      'tight-ink-right', 'tight-ink-left',
    ]) {
      dom.window.scrcpyControls.setLayout(layout);
      expect(document.body.dataset.layout).toBe(layout);
      expect(dom.window.getComputedStyle(document.querySelector('#ink-tools')).display).toBe('flex');
    }
    dom.window.scrcpyControls.setLayout('tight-ink-right', 65);
    expect(document.documentElement.style.zoom).toBe('0.65');
    expect(document.querySelectorAll('.palette-actions button')).toHaveLength(4);
    expect(document.querySelectorAll('.swatches button')).toHaveLength(4);
    expect(document.querySelectorAll('.sizes button')).toHaveLength(3);
    dom.window.scrcpyControls.setLayout('short-ink-left', 100);
    expect(document.documentElement.style.zoom).toBe('1');
    dom.window.scrcpyControls.setLayout('short-ink-left', -9);
    expect(document.documentElement.style.zoom).toBe('1');
    dom.window.scrcpyControls.setLayout('invalid-layout');
    expect(document.body.dataset.layout).toBe('short-ink-left');
    expect(document.documentElement.style.zoom).toBe('1');
    dom.window.close();
  });

  it('offers creator shapes and warns masks do not protect raw recording',async()=>{
    const {dom,document,actions}=setup();
    document.getElementById('ink-toggle').click();
    await nextTask();await nextTask();
    document.getElementById('ink-creator-tab').click();
    await nextTask();
    expect(document.body.dataset.inkPage).toBe('creator');
    expect(actions).toContain('ink_arrow');
    for (const action of ['rect','step','mask']) {
      document.getElementById('creator-'+action).click();
      await nextTask();
      expect(actions).toContain('ink_'+action);
      expect(document.body.dataset.inkTool).toBe(action);
    }
    expect(document.getElementById('ink-privacy-note').textContent)
      .toMatch(/原始录屏\/OBS/);
    expect(document.getElementById('creator-mask').getAttribute('aria-pressed')).toBe('true');
    document.getElementById('ink-basic-tab').click();
    await nextTask();
    expect(document.body.dataset.inkTool).toBe('pen');
    document.getElementById('restore').click();
    await nextTask();
    expect(document.body.dataset.inkPage).toBe('basic');
    dom.window.close();
  });

  it('click effects stay native mouse-through and live lens is opt-in',async()=>{
    const {dom,document,actions}=setup();
    document.getElementById('present-toggle').click();
    await nextTask();await nextTask();
    document.getElementById('creator-click').click();
    await nextTask();await nextTask();
    expect(actions).toContain('creator_click_toggle');
    expect(document.getElementById('creator-click').getAttribute('aria-pressed')).toBe('true');
    // A native callback is emitted ONLY after the WebView canvas confirms it
    // processed the actual click. The count distinguishes missing input from
    // missing visual compositing.
    expect(dom.window.mirroCreatorNativeClick()).toBe(true);
    expect(dom.window.mirroCreatorNativeClick()).toBe(true);
    expect(document.getElementById('creator-click').dataset.nativeClicks).toBe('2');
    expect(document.getElementById('present-feedback')).toHaveTextContent('已识别 2 次点击');
    document.getElementById('present-live').click();
    await nextTask();await nextTask();
    expect(actions).toContain('present_live');
    expect(document.getElementById('present-live').getAttribute('aria-pressed')).toBe('true');
    expect(document.getElementById('present-hint')).toHaveTextContent('滚轮调圈大小');
    document.getElementById('present-off').click();
    await nextTask();await nextTask();
    expect(document.getElementById('present-live').getAttribute('aria-pressed')).toBe('false');
    expect(document.getElementById('present-hint')).toHaveTextContent('定格和聚光灯暂停触控');
    dom.window.close();
  });

  it('shows bounded Android quick controls with native acknowledgement', async () => {
    const { dom, document, actions } = setup();
    document.getElementById('quick-toggle').click();
    await nextTask(); await nextTask();
    expect(document.body.classList.contains('quick-open')).toBe(true);
    expect(document.getElementById('quick-toggle').getAttribute('aria-pressed')).toBe('true');
    for (const action of ['fullscreen', 'home', 'back', 'recents', 'power', 'rotate', 'paste']) {
      document.getElementById('quick-' + action).click();
      await nextTask();
    }
    expect(actions).toEqual(expect.arrayContaining([
      'quick_fullscreen','quick_home','quick_back','quick_recents','quick_power',
      'quick_rotate','quick_paste',
    ]));
    const top = document.getElementById('quick-top');
    expect(top).not.toBeNull();
    expect(top.getAttribute('aria-pressed')).toBe('false');
    top.click();
    await nextTask(); await nextTask();
    expect(actions).toContain('quick_top_toggle');
    expect(top.getAttribute('aria-pressed')).toBe('true');
    top.click();
    await nextTask(); await nextTask();
    expect(top.getAttribute('aria-pressed')).toBe('false');
    const rememberButton = document.getElementById('quick-remember');
    expect(rememberButton.getAttribute('aria-pressed')).toBe('true');
    rememberButton.click();
    await nextTask(); await nextTask();
    expect(rememberButton.getAttribute('aria-pressed')).toBe('false');
    expect(actions).toContain('quick_remember_toggle');
    rememberButton.click();
    await nextTask(); await nextTask();
    expect(rememberButton.getAttribute('aria-pressed')).toBe('true');
    document.getElementById('ink-toggle').click();
    await nextTask(); await nextTask();
    expect(document.body.classList.contains('quick-open')).toBe(false);
    expect(document.body.classList.contains('inking')).toBe(true);
    dom.window.close();
  });

  it('shows verified pin state immediately and keeps its green indicator after feedback', async () => {
    const {dom, document, actions} = setup();
    document.getElementById('quick-toggle').click();
    await nextTask(); await nextTask();
    const top = document.getElementById('quick-top');
    expect(top.getAttribute('aria-pressed')).toBe('false');
    expect(document.getElementById('quick-top-label').textContent).toBe('置顶');
    expect(document.getElementById('quick-feedback').textContent)
      .toContain('普通窗口');
    top.click();
    top.click(); // one in-flight user click must produce only one native request
    await nextTask(); await nextTask();
    expect(actions.filter(action => action === 'quick_top_toggle')).toHaveLength(1);
    expect(top.getAttribute('aria-pressed')).toBe('true');
    expect(top.classList.contains('active')).toBe(true);
    expect(document.getElementById('quick-top-label').textContent).toBe('已置顶');
    expect(top.getAttribute('aria-label')).toBe('取消窗口置顶');
    expect(dom.window.getComputedStyle(top).backgroundColor).toMatch(/18,\s*185,\s*151/);
    expect(document.getElementById('quick-feedback').textContent)
      .toContain('没有中断');
    await new Promise(resolve => setTimeout(resolve, 3550));
    expect(document.getElementById('quick-feedback').textContent)
      .toContain('已置顶');
    expect(top.classList.contains('active')).toBe(true);
    top.click();
    await nextTask(); await nextTask();
    expect(top.getAttribute('aria-pressed')).toBe('false');
    expect(document.getElementById('quick-top-label').textContent).toBe('置顶');
    dom.window.close();
  });

  it('shows a native topmost failure without claiming success or toggling the button', async () => {
    const {dom, document, actions} = setup({
      failAction:'quick_top_toggle',
      failReason:'Windows refused the live HWND z-order change',
    });
    document.getElementById('quick-toggle').click();
    await nextTask(); await nextTask();
    const top = document.getElementById('quick-top');
    top.click();
    expect(top.dataset.feedback).toBe('busy');
    await nextTask(); await nextTask();
    expect(top.getAttribute('aria-pressed')).toBe('false');
    expect(top.classList.contains('active')).toBe(false);
    expect(document.getElementById('quick-feedback').textContent).toContain('实时置顶失败');
    expect(document.getElementById('quick-feedback').textContent).toContain('Windows refused');
    expect(actions).toContain('quick_top_status');
    expect(actions).not.toContain('stop_scrcpy');
    dom.window.close();
  });

  it('resynchronizes the real topmost state when the toolbar is reopened', async () => {
    const {dom, document, setTop} = setup();
    document.getElementById('quick-toggle').click();
    await nextTask(); await nextTask();
    setTop(true); // Native tier changed after a Windows restore.
    dom.window.scrcpyControls.sync();
    await nextTask(); await nextTask();
    const top = document.getElementById('quick-top');
    expect(top.getAttribute('aria-pressed')).toBe('true');
    expect(document.getElementById('quick-top-label').textContent).toBe('已置顶');
    setTop(false);
    dom.window.dispatchEvent(new dom.window.Event('focus'));
    await nextTask(); await nextTask();
    expect(top.getAttribute('aria-pressed')).toBe('false');
    dom.window.close();
  });

  it('displays pending, dispatched and failure feedback for shortcut actions', async () => {
    const { dom, document } = setup({failAction:'quick_paste'});
    document.getElementById('quick-toggle').click();
    await nextTask(); await nextTask();
    const rotate = document.getElementById('quick-rotate');
    rotate.click();
    expect(rotate.dataset.feedback).toBe('busy');
    expect(rotate.getAttribute('aria-busy')).toBe('true');
    await nextTask();
    expect(rotate.dataset.feedback).toBe('sent');
    expect(document.getElementById('quick-feedback').textContent)
      .toContain('已确认 Android 画面方向切换');
    const failed = document.getElementById('quick-paste');
    failed.click();
    await nextTask();
    expect(failed.dataset.feedback).toBe('error');
    expect(document.getElementById('quick-feedback').textContent)
      .toContain('粘贴失败');
    expect(document.getElementById('status').dataset.error).toBe('true');
    dom.window.close();
  });

  it('shows an Android orientation-lock failure rather than success', async () => {
    const {dom, document} = setup({
      failAction: 'quick_rotate',
      failReason: '手机当前应用没有切换方向。请确认该应用支持横屏并允许自动旋转。',
    });
    document.getElementById('quick-toggle').click();
    await nextTask(); await nextTask();
    const rotate = document.getElementById('quick-rotate');
    rotate.click();
    expect(rotate.dataset.feedback).toBe('busy');
    expect(document.getElementById('quick-feedback').textContent)
      .toContain('正在请求 Android 切换方向');
    await nextTask();
    expect(rotate.dataset.feedback).toBe('error');
    expect(document.getElementById('quick-feedback').textContent)
      .toContain('手机当前应用没有切换方向');
    expect(document.getElementById('status').dataset.error).toBe('true');
    dom.window.close();
  });

  it('opens presenter palette, switches spotlight and still lens, and safely exits', async () => {
    const {dom,document,actions}=setup();
    const toggle=document.getElementById('present-toggle');
    toggle.click();
    await nextTask();await nextTask();
    expect(document.body.classList.contains('present-open')).toBe(true);
    expect(document.getElementById('present-refresh').disabled).toBe(true);
    document.getElementById('present-spotlight').click();
    await nextTask();await nextTask();
    expect(document.getElementById('present-spotlight').getAttribute('aria-pressed')).toBe('true');
    document.getElementById('present-magnifier').click();
    await nextTask();await nextTask();
    expect(document.getElementById('present-magnifier').getAttribute('aria-pressed')).toBe('true');
    expect(document.getElementById('present-refresh').disabled).toBe(false);
    document.getElementById('present-refresh').click();
    await nextTask();await nextTask();
    expect(actions).toContain('present_refresh');
    document.getElementById('present-off').click();
    await nextTask();await nextTask();
    expect(document.getElementById('present-magnifier').getAttribute('aria-pressed')).toBe('false');
    toggle.click();
    await nextTask();await nextTask();
    expect(document.body.classList.contains('present-open')).toBe(false);
    expect(actions).toEqual(expect.arrayContaining([
      'present_toggle','present_spotlight','present_magnifier','present_refresh','present_off'
    ]));
    dom.window.close();
  });

  it('keeps drawing, quick controls and MirroPresent mutually exclusive', async () => {
    const {dom,document}=setup();
    document.getElementById('present-toggle').click();
    await nextTask();await nextTask();
    document.getElementById('quick-toggle').click();
    await nextTask();await nextTask();
    expect(document.body.classList.contains('quick-open')).toBe(true);
    expect(document.body.classList.contains('present-open')).toBe(false);
    document.getElementById('present-toggle').click();
    await nextTask();await nextTask();
    expect(document.body.classList.contains('quick-open')).toBe(false);
    document.getElementById('ink-toggle').click();
    await nextTask();await nextTask();
    expect(document.body.classList.contains('present-open')).toBe(false);
    expect(document.body.classList.contains('inking')).toBe(true);
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
