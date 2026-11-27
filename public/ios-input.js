(() => {
  'use strict';
  if (!window.__MIRROLEAP_AIRPLAY__) return;

  const surface = document.getElementById('ios-input');
  const usages = {
    Enter:0x28, Escape:0x29, Backspace:0x2a, Tab:0x2b, Space:0x2c,
    Minus:0x2d, Equal:0x2e, BracketLeft:0x2f, BracketRight:0x30, Backslash:0x31,
    Semicolon:0x33, Quote:0x34, Backquote:0x35, Comma:0x36, Period:0x37, Slash:0x38,
    CapsLock:0x39, PrintScreen:0x46, ScrollLock:0x47, Pause:0x48,
    Insert:0x49, Home:0x4a, PageUp:0x4b, Delete:0x4c, End:0x4d, PageDown:0x4e,
    ArrowRight:0x4f, ArrowLeft:0x50, ArrowDown:0x51, ArrowUp:0x52, NumLock:0x53,
    NumpadDivide:0x54, NumpadMultiply:0x55, NumpadSubtract:0x56, NumpadAdd:0x57,
    NumpadEnter:0x58, Numpad0:0x62, NumpadDecimal:0x63, IntlBackslash:0x64, ContextMenu:0x65,
    ControlLeft:0xe0, ShiftLeft:0xe1, AltLeft:0xe2, MetaLeft:0xe3,
    ControlRight:0xe4, ShiftRight:0xe5, AltRight:0xe6, MetaRight:0xe7,
  };
  for (const [index, letter] of [...'ABCDEFGHIJKLMNOPQRSTUVWXYZ'].entries()) usages['Key'+letter] = 4 + index;
  for (const [index, digit] of [...'1234567890'].entries()) usages['Digit'+digit] = 0x1e + index;
  for (let key = 1; key <= 12; key++) usages['F'+key] = 0x39 + key;
  for (let key = 1; key <= 9; key++) usages['Numpad'+key] = 0x58 + key;

  let queue = [];
  const pressed = new Set();
  const deferredAlt = new Set();
  const pendingClicks = new Map();
  let enabled = false;
  let hovered = false;
  let focused = false;
  let sending = false;
  let scheduled = false;
  let buttons = 0;
  let wheelRemainder = 0;
  let swipe = null;
  let holdTimer = 0;
  const swipeThreshold = 12;
  const pixelsPerScrollStep = 18;
  const maxSwipeSteps = 8;
  const holdToSelectMs = 320;

  async function drain() {
    if (sending || !queue.length) return;
    sending = true;
    try {
      const events = queue.splice(0, 12).map(entry => entry.event);
      await window.__TAURI__.core.invoke('device_frame_ios_input', {events});
    } catch {
      // Any failed batch may have reached the iPad in part. Discard the
      // remaining gestures and request an all-up before accepting new input.
      if (hovered || pressed.size || buttons) leave(true);
    } finally {
      sending = false;
      if (queue.length) void drain();
    }
  }

  function enqueue(event, move = false) {
    const tail = queue.at(-1);
    // A click or swipe already carries its own position. Don't send a stale
    // hover update before it, or make typing wait for that update.
    if (!move && tail?.move && tail.event.buttons === 0) queue.pop();
    const last = queue.at(-1);
    const scroll = event.kind === 'scroll';
    if (move && last?.move) {
      queue[queue.length - 1] = {event, move};
    } else if (scroll && !event.start && last?.event.kind === 'scroll' &&
        last.event.x === event.x && last.event.y === event.y &&
        Math.abs(last.event.wheel + event.wheel) <= maxSwipeSteps &&
        Math.abs(last.event.pan + event.pan) <= maxSwipeSteps &&
        (last.event.wheel * event.wheel > 0 || last.event.pan * event.pan > 0)) {
      last.event.wheel += event.wheel;
      last.event.pan += event.pan;
    } else if (queue.length < 96) {
      queue.push({event, move});
    } else {
      leave(true);
      return;
    }
    if (!move && !scroll) queueMicrotask(drain);
    else if (!scheduled) {
      scheduled = true;
      requestAnimationFrame(() => { scheduled = false; void drain(); });
    }
  }

  function cancelHold() {
    clearTimeout(holdTimer);
    holdTimer = 0;
  }

  function leave(discard = false) {
    const needsRelease = hovered || pressed.size || buttons || focused;
    hovered = false;
    focused = false;
    buttons = 0;
    swipe = null;
    cancelHold();
    pendingClicks.clear();
    pressed.clear();
    deferredAlt.clear();
    wheelRemainder = 0;
    if (discard) {
      queue.length = 0;
      queue.push({event:{kind:'release'}, move:false});
      void drain();
    } else if (needsRelease) {
      queue = queue.filter(entry => entry.event.kind !== 'scroll');
      enqueue({kind:'release'});
    }
  }

  function point(event) {
    const rect = surface.getBoundingClientRect();
    if (!enabled || !rect.width || !rect.height ||
        event.clientX < rect.left || event.clientX >= rect.right ||
        event.clientY < rect.top || event.clientY >= rect.bottom) return null;
    return {
      x: Math.round((event.clientX - rect.left) / rect.width * 10000),
      y: Math.round((event.clientY - rect.top) / rect.height * 10000),
    };
  }

  function pointer(event, move, wheel = 0) {
    const position = point(event);
    if (!position) { leave(); return; }
    hovered = true;
    enqueue({kind:'pointer', ...position, buttons, wheel}, move);
  }

  function scrollSwipe(event) {
    const coordinate = swipe.horizontal ? event.clientX : event.clientY;
    swipe.remainder += coordinate - swipe.coordinate;
    swipe.coordinate = coordinate;
    const wholeSteps = Math.trunc(swipe.remainder / pixelsPerScrollStep);
    if (!wholeSteps) return;
    swipe.remainder -= wholeSteps * pixelsPerScrollStep;
    const steps = Math.max(-maxSwipeSteps, Math.min(maxSwipeSteps, wholeSteps));
    enqueue({
      kind:'scroll', ...swipe.position,
      start:!swipe.sent,
      wheel:swipe.horizontal ? 0 : -steps,
      pan:swipe.horizontal ? steps : 0,
    });
    swipe.sent = true;
  }

  function startSwipe(event, start) {
    const dx = event.clientX - start.clientX;
    const dy = event.clientY - start.clientY;
    const horizontal = Math.abs(dx) > Math.abs(dy);
    const distance = horizontal ? dx : dy;
    if (Math.abs(distance) < swipeThreshold) return false;
    cancelHold();
    swipe = {horizontal, position:start.position, coordinate:horizontal ? start.clientX : start.clientY,
      remainder:Math.sign(distance) * (pixelsPerScrollStep - swipeThreshold), sent:false};
    pendingClicks.clear();
    scrollSwipe(event);
    return true;
  }

  function beginDrag(button, start) {
    pendingClicks.delete(button);
    buttons |= button;
    enqueue({kind:'pointer', ...start.position, buttons, wheel:0});
  }

  surface.addEventListener('mousemove', event => {
    const position = point(event);
    if (!position) { leave(); return; }
    if (swipe && (event.buttons & 1)) {
      scrollSwipe(event);
      return;
    }
    swipe = null;
    if ((buttons & event.buttons) !== buttons) {
      buttons &= event.buttons;
      pointer(event, false);
    }
    for (const [button, start] of pendingClicks) {
      if (!(event.buttons & button)) {
        if (button === 1) cancelHold();
        pendingClicks.delete(button);
        continue;
      }
      if (start.scroll) {
        startSwipe(event, start);
        return;
      }
      if (Math.hypot(event.clientX - start.clientX, event.clientY - start.clientY) >= 4) {
        beginDrag(button, start);
      }
    }
    pointer(event, true);
  });
  surface.addEventListener('mouseenter', event => pointer(event, true));
  surface.addEventListener('mouseleave', () => leave());
  surface.addEventListener('mousedown', event => {
    const mask = [1, 4, 2][event.button];
    const position = point(event);
    if (!mask || !position) return;
    event.preventDefault();
    surface.focus({preventScroll:true});
    focused = true;
    hovered = true;
    if (!swipe) {
      const start = {
        position, clientX:event.clientX, clientY:event.clientY,
        selection:mask === 1 && event.altKey,
        scroll:mask === 1 && !event.altKey && buttons === 0 && pendingClicks.size === 0,
      };
      pendingClicks.set(mask, start);
      if (start.selection) {
        // Keep the Windows selection gesture local even if Alt was used for
        // a forwarded keyboard shortcut immediately before this press.
        for (const usage of [0xe2, 0xe6]) {
          if (pressed.delete(usage)) {
            deferredAlt.add(usage);
            enqueue({kind:'key', usage, down:false});
          }
        }
      }
      if (start.scroll) {
        cancelHold();
        holdTimer = setTimeout(() => {
          holdTimer = 0;
          if (pendingClicks.get(1) === start) beginDrag(1, start);
        }, holdToSelectMs);
      }
    }
  });
  window.addEventListener('mouseup', event => {
    const mask = [1, 4, 2][event.button];
    if (mask === 1) cancelHold();
    const start = pendingClicks.get(mask);
    if (mask === 1 && (swipe || (start?.scroll && point(event) && startSwipe(event, start)))) {
      event.preventDefault();
      if (point(event)) scrollSwipe(event);
      swipe = null;
      pointer(event, false);
      return;
    }
    if (!mask || !(buttons & mask) && !pendingClicks.has(mask)) return;
    event.preventDefault();
    if (pendingClicks.delete(mask)) {
      if (point(event)) enqueue({kind:'click', ...start.position, button:mask});
      else leave();
    } else {
      buttons &= ~mask;
      pointer(event, false);
    }
  }, true);
  surface.addEventListener('contextmenu', event => event.preventDefault());
  surface.addEventListener('wheel', event => {
    if (!point(event)) return;
    event.preventDefault();
    const units = event.deltaMode === 0 ? 100 : event.deltaMode === 1 ? 3 : 1;
    wheelRemainder += event.deltaY / units;
    const steps = Math.max(-127, Math.min(127, Math.round(wheelRemainder)));
    wheelRemainder -= steps;
    if (steps) pointer(event, false, steps);
  }, {passive:false});

  function keyboard(event, down) {
    const usage = usages[event.code];
    if (!enabled || !focused || !hovered || !usage) return;
    event.preventDefault();
    if (usage === 0xe2 || usage === 0xe6) {
      if (down) {
        if (!pressed.has(usage)) deferredAlt.add(usage);
        return;
      }
      if (deferredAlt.delete(usage)) return;
    }
    if (down === pressed.has(usage)) return;
    if (down && event.code === 'Escape') {
      buttons = 0;
      swipe = null;
      cancelHold();
      pendingClicks.clear();
      queue = queue.filter(entry => entry.event.kind !== 'scroll');
    } else if (down && event.altKey && !(buttons & 1) && !pendingClicks.get(1)?.selection) {
      for (const alt of deferredAlt) {
        pressed.add(alt);
        enqueue({kind:'key', usage:alt, down:true});
      }
      deferredAlt.clear();
    }
    if (down) pressed.add(usage);
    else pressed.delete(usage);
    enqueue({kind:'key', usage, down});
  }
  window.addEventListener('keydown', event => keyboard(event, true));
  window.addEventListener('keyup', event => keyboard(event, false));
  window.addEventListener('blur', () => leave());
  window.addEventListener('resize', () => {
    if (hovered || buttons || pressed.size) leave(true);
  });
  document.addEventListener('visibilitychange', () => { if (document.hidden) leave(); });

  window.mirroleapIosInput = Object.freeze({
    release() { leave(); },
    setEnabled(active) {
      if (enabled && !active && (hovered || pressed.size || buttons || queue.length)) leave(true);
      enabled = active;
      document.body.classList.toggle('ios-input', active);
      document.body.setAttribute('aria-hidden', String(!active));
    },
  });
})();
