// Native first-paint handshake for the hidden Windows phone-frame WebView2.
// It is not enough for navigation to finish: a default white WebView surface
// can cover the SDL video before the SVG/Canvas and transparent background load.
(() => {
  'use strict';
  const frame = document.getElementById('device-frame');
  const annotation = document.getElementById('annotation');
  const presentation = document.getElementById('presentation');
  const transparent = element => {
    if (!element) return false;
    const background = getComputedStyle(element).backgroundColor.replace(/\s+/g, '');
    return background === 'transparent' || /^rgba\(\d+,\d+,\d+,0(?:\.0+)?\)$/.test(background);
  };
  const paintedOrHidden = () => new Promise(resolve => {
    // rAF is usually SUSPENDED while the native HWND is hidden. A bounded
    // fallback avoids a startup deadlock without showing an unready page.
    let completed = false;
    const finish = () => {
      if (completed) return;
      completed = true;
      clearTimeout(fallback);
      resolve();
    };
    const fallback = setTimeout(finish, 240);
    requestAnimationFrame(() => requestAnimationFrame(finish));
  });
  async function acknowledge() {
    if (!frame || !annotation || !presentation) return;
    const invoke = window.__TAURI__?.core?.invoke;
    if (typeof invoke !== 'function') return;
    try {
      if (typeof frame.decode === 'function') {
        await frame.decode();
      } else if (!frame.complete) {
        await new Promise((resolve,reject) => {
          frame.addEventListener('load',resolve,{once:true});
          frame.addEventListener('error',()=>reject(Error('Frame SVG unavailable')),{once:true});
        });
      }
      if (!frame.complete || frame.naturalWidth < 1 ||
          !window.scrcpyInk?.ready || !window.scrcpyPresent?.ready ||
          ![document.documentElement,document.body,annotation,presentation].every(transparent)) {
        throw Error('Transparent phone-frame canvas is not ready');
      }
      await paintedOrHidden();
      await invoke('device_frame_page_ready');
    } catch (error) {
      // Do NOT signal success or reveal an opaque fallback page on failure.
      console.error('[MirroLeap frame] Transparent first paint unavailable', error);
    }
  }
  void acknowledge();
})();
