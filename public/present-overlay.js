// MirroPresent stays independent of the pen Canvas, native SDL video and
// floating window chrome. It never contains the scrcpy video stream:
// spotlight is shading; magnifier is an explicit still screenshot; "live"
// receives newly captured Android screenshots at a deliberately capped rate
// (not 60 FPS or a hardware-accelerated clone of the native SDL video).
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
    if (mode === 'off' || w < 1 || h < 1) return;
    const x = point.x * w;
    const y = point.y * h;
    const radius = Math.max(12, Math.min(w, h) * radiusRatio);
    ctx.save();
    if (mode === 'spotlight') {
      ctx.fillStyle = 'rgba(5,9,20,.73)';
      ctx.fillRect(0, 0, w, h);
      ctx.globalCompositeOperation = 'destination-out';
      ctx.beginPath();
      ctx.arc(x, y, radius, 0, Math.PI * 2);
      ctx.fill();
      ctx.globalCompositeOperation = 'source-over';
    } else if ((mode === 'magnifier' || mode === 'live') && snapshot) {
      const zoom = zoomFactor;
      const sampleW = (radius * 2 / zoom / w) * snapshot.width;
      const sampleH = (radius * 2 / zoom / h) * snapshot.height;
      const srcX = Math.max(0, Math.min(snapshot.width - sampleW,
        point.x * snapshot.width - sampleW / 2));
      const srcY = Math.max(0, Math.min(snapshot.height - sampleH,
        point.y * snapshot.height - sampleH / 2));
      ctx.beginPath();
      ctx.arc(x, y, radius, 0, Math.PI * 2);
      ctx.clip();
      ctx.drawImage(snapshot, srcX, srcY, sampleW, sampleH,
        x - radius, y - radius, radius * 2, radius * 2);
      ctx.restore();
      ctx.save();
    }
    ctx.strokeStyle = '#4de9de';
    ctx.lineWidth = Math.max(2, Math.min(w, h) / 180);
    ctx.shadowColor = 'rgba(0,0,0,.7)';
    ctx.shadowBlur = 6;
    ctx.beginPath();
    ctx.arc(x, y, radius, 0, Math.PI * 2);
    ctx.stroke();
    ctx.restore();
    if (mode === 'magnifier' || mode === 'live') {
      ctx.save();
      ctx.font = '600 12px sans-serif';
      ctx.textAlign = 'center';
      ctx.fillStyle = '#fff';
      ctx.strokeStyle = '#07121b';
      ctx.lineWidth = 3;
      const label = mode === 'live'
        ? (snapshot ? '连续画面放大 · ADB 低帧率' : '读取实时采样中…')
        : (snapshot ? '定格放大 · 非实时' : '请刷新放大快照');
      const yText = Math.min(h - 12, Math.max(18, y + radius + 18));
      ctx.strokeText(label, x, yText);
      ctx.fillText(label, x, yText);
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
    if (event.ctrlKey && (mode === 'magnifier' || mode === 'live')) {
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
      document.body.classList.toggle('presenting', value !== 'off');
      fit();
      return mode;
    },
    async loadSnapshot(dataUrl, orientation) {
      if (mode !== 'magnifier' && mode !== 'live') throw new Error('Enable a magnifier first');
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
      if (token !== snapshotToken ||
          (mode !== 'magnifier' && mode !== 'live') ||
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
