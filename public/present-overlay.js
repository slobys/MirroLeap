// MirroPresent stays independent of the pen Canvas, native SDL video and
// floating window chrome. Spotlight is shading; magnifier uses an Android
// still screenshot. LIVE is a separate mouse-through native Windows
// Magnification control over the SDL video, not a stream of ADB PNGs.
(() => {
  'use strict';
  const canvas = document.getElementById('presentation');
  if (!canvas) return;
  const ctx = canvas.getContext('2d', {alpha: true});
  if (!ctx) return;
  const modes = new Set(['off', 'spotlight', 'magnifier', 'live']);
  const clamp = value => Math.max(0, Math.min(1, value));
  let mode = 'off';
  let point = {x: .5, y: .5};
  let snapshot = null;
  let snapshotToken = 0;

  let drawPending = false;
  const minRadiusRatio = .11;
  const maxRadiusRatio = .43;
  const defaultRadiusRatio = .22;
  let radiusRatio = defaultRadiusRatio;
  const zoomDefault = 2.3;
  let zoomFactor = zoomDefault;
  const adjustable = (value, low, high) => Math.max(low, Math.min(high, value));

  function paint() {
    drawPending = false;
    const w = canvas.width, h = canvas.height;
    ctx.clearRect(0, 0, w, h);
    // The native live lens draws itself directly from the Windows compositor.
    // Never paint a second fake/static magnifier on top of it.
    if (mode === 'off' || mode === 'live' || w < 1 || h < 1) return;
    const x = point.x * w;
    const y = point.y * h;
    const radius = Math.max(12, Math.min(w, h) * radiusRatio);
    // The focus stays at the Android coordinate under the pointer, but the
    // *visible circle* moves inward when an edge icon is selected.
    const margin = Math.max(6, Math.min(w, h) * .012);
    const lensX = mode === 'magnifier'
      ? adjustable(x, radius + margin, w - radius - margin) : x;
    const lensY = mode === 'magnifier'
      ? adjustable(y, radius + margin, h - radius - margin) : y;
    ctx.save();
    if (mode === 'spotlight') {
      ctx.fillStyle = 'rgba(5,9,20,.73)';
      ctx.fillRect(0, 0, w, h);
      ctx.globalCompositeOperation = 'destination-out';
      ctx.beginPath();
      ctx.arc(x, y, radius, 0, Math.PI * 2);
      ctx.fill();
      ctx.globalCompositeOperation = 'source-over';
    } else if (mode === 'magnifier' && snapshot) {
      const sampleW = (radius * 2 / zoomFactor / w) * snapshot.width;
      const sampleH = (radius * 2 / zoomFactor / h) * snapshot.height;
      // Do not clamp the requested source center to the image edge: that
      // pushes rightmost icons onto the lens rim and clips them a second time.
      // Instead fill out-of-image areas and map the valid PNG pixels into
      // the interior, keeping the selected icon close to the lens center.
      const left = point.x * snapshot.width - sampleW / 2;
      const top = point.y * snapshot.height - sampleH / 2;
      const sx = Math.max(0, left);
      const sy = Math.max(0, top);
      const sw = Math.max(0, Math.min(snapshot.width, left + sampleW) - sx);
      const sh = Math.max(0, Math.min(snapshot.height, top + sampleH) - sy);
      ctx.beginPath();
      ctx.arc(lensX, lensY, radius, 0, Math.PI * 2);
      ctx.clip();
      ctx.fillStyle = '#09111d';
      ctx.fillRect(lensX - radius, lensY - radius, radius * 2, radius * 2);
      if (sw > 0 && sh > 0) {
        ctx.drawImage(snapshot, sx, sy, sw, sh,
          lensX - radius + (sx - left) / sampleW * radius * 2,
          lensY - radius + (sy - top) / sampleH * radius * 2,
          sw / sampleW * radius * 2,
          sh / sampleH * radius * 2);
      }
      ctx.restore();
      ctx.save();
    }
    ctx.strokeStyle = '#4de9de';
    ctx.lineWidth = Math.max(2, Math.min(w, h) / 180);
    ctx.shadowColor = 'rgba(0,0,0,.7)';
    ctx.shadowBlur = 6;
    ctx.beginPath();
    ctx.arc(lensX, lensY, radius, 0, Math.PI * 2);
    ctx.stroke();
    ctx.restore();
    if (mode === 'magnifier') {
      ctx.save();
      ctx.font = '600 12px sans-serif';
      ctx.textAlign = 'center';
      ctx.fillStyle = '#fff';
      ctx.strokeStyle = '#07121b';
      ctx.lineWidth = 3;
      const label = snapshot ? '定格放大 · 非实时' : '请刷新放大快照';
      const yText = lensY + radius + 24 < h ? lensY + radius + 18 : lensY - radius - 12;
      ctx.strokeText(label, lensX, yText, Math.max(60,w - 20));
      ctx.fillText(label, lensX, yText, Math.max(60,w - 20));
      ctx.restore();
    }
  }

  function redraw() {
    if (!drawPending) {
      drawPending = true;
      requestAnimationFrame(paint);
    }
  }

  function fit() {
    const rect = canvas.getBoundingClientRect();
    const dpr = Math.max(1, Math.min(3, window.devicePixelRatio || 1));
    const width = Math.max(1, Math.round(rect.width * dpr));
    const height = Math.max(1, Math.round(rect.height * dpr));
    if (canvas.width !== width || canvas.height !== height) {
      canvas.width = width;
      canvas.height = height;
    }
    redraw();
  }
  const currentOrientation = () =>
    document.documentElement.dataset.orientation || 'portrait';

  // The frame may rotate *without* a browser resize. A screenshot taken in
  // the previous orientation can no longer be mapped onto the new view.
  const observer = new MutationObserver(records => {
    if (records.some(record => record.attributeName === 'data-orientation')) {
      snapshot = null;
      snapshotToken++;
      fit();
    }
  });
  observer.observe(document.documentElement, {
    attributes: true, attributeFilter: ['data-orientation']
  });
  new ResizeObserver(fit).observe(canvas);
  window.addEventListener('resize', fit);

  canvas.addEventListener('pointermove', event => {
    if (mode === 'off') return;
    const rect = canvas.getBoundingClientRect();
    point = {
      x: clamp((event.clientX - rect.left) / Math.max(1, rect.width)),
      y: clamp((event.clientY - rect.top) / Math.max(1, rect.height)),
    };
    redraw();
  });
  canvas.addEventListener('pointerdown', event => {
    if (mode !== 'off') event.preventDefault();
  });
  // Resize the spotlight / snapshot lens directly under the mouse cursor.
  // No browser zoom, native mirror resize, or change to saved pen layers.
  canvas.addEventListener('wheel', event => {
    if (mode === 'off') return;
    event.preventDefault();
    if (event.ctrlKey && mode === 'magnifier') {
      zoomFactor = adjustable(
        zoomFactor + (event.deltaY < 0 ? .15 : -.15), 1.4, 4.2);
    } else {
      radiusRatio = adjustable(
        Math.round((radiusRatio + (event.deltaY < 0 ? .025 : -.025)) * 1000) / 1000,
        minRadiusRatio, maxRadiusRatio);
    }
    redraw();
  }, {passive:false});

  // Emergency exit can be triggered from the existing phone hover toolbar's
  // one-click restore, even if the main UI is behind another program.
  window.scrcpyPresent = Object.freeze({
    ready: true,
    mode() { return mode; },
    focusPoint() { return {...point}; },
    size() { return radiusRatio; },
    zoom() { return zoomFactor; },
    resetSize() {
      radiusRatio = defaultRadiusRatio;
      zoomFactor = zoomDefault;
      redraw();
      return radiusRatio;
    },
    setMode(value) {
      if (!modes.has(value)) throw new Error('Unknown presentation mode');
      if (value !== mode) {
        snapshot = null;
        snapshotToken++;
      }
      mode = value;
      // Only these modes intentionally capture the pointer. The native
      // live lens uses a separate WS_EX_TRANSPARENT HWND.
      document.body.classList.toggle('presenting', value === 'spotlight' || value === 'magnifier');
      fit();
      return mode;
    },
    async loadSnapshot(dataUrl, orientation) {
      if (mode !== 'magnifier') throw new Error('Enable snapshot magnifier first');
      if (typeof dataUrl !== 'string' || !dataUrl.startsWith('data:image/png;base64,') ||
          dataUrl.length > 17_000_000) {
        throw new Error('Invalid or oversized PNG snapshot');
      }
      if (orientation !== currentOrientation()) {
        throw new Error('Phone orientation changed while taking screenshot');
      }
      const token = ++snapshotToken;
      const photo = await new Promise((resolve, reject) => {
        const image = new Image();
        image.onload = () => resolve(image);
        image.onerror = () => reject(new Error('Phone snapshot decoding failed'));
        image.src = dataUrl;
      });
      if (token !== snapshotToken || mode !== 'magnifier' ||
          currentOrientation() !== orientation) {
        throw new Error('Presentation mode or phone orientation changed during capture');
      }
      if (!photo.width || !photo.height || photo.width * photo.height > 16_000_000) {
        throw new Error('Screenshot dimensions are not supported');
      }
      fit();
      const ratio = photo.width / photo.height;
      const display = canvas.width / canvas.height;
      if (Math.abs(ratio / display - 1) > .06) {
        throw new Error('Video aspect ratio changed; refresh after rotation settles');
      }
      snapshot = photo;
      redraw();
      return true;
    },
    clearSnapshot() {
      snapshot = null;
      snapshotToken++;
      redraw();
    },
  });
  fit();
})();
