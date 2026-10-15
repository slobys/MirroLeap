#!/usr/bin/env node
// Windows WebView2 STARTUP regression: inspect the actual running Tauri
// frontend over Chromium DevTools Protocol. npm test / Vite build can pass
// while the installed program displays a completely blank main window.
// Only used on ephemeral GitHub Windows runners with a loopback-only port.
const endpoint = process.env.MIRROLEAP_SMOKE_ENDPOINT || 'http://127.0.0.1:19222';
const durationMs = Number(process.env.MIRROLEAP_SMOKE_TIMEOUT_MS || 55_000);
const deadline = Date.now() + durationMs;
const pause = (ms) => new Promise((resolve) => setTimeout(resolve, ms));

async function evaluate(target) {
  return new Promise((resolve, reject) => {
    const ws = new WebSocket(target.webSocketDebuggerUrl);
    const t = setTimeout(() => { ws.close(); reject(Error('CDP timed out')); }, 4000);
    ws.addEventListener('open', () => {
      ws.send(JSON.stringify({
        id: 1,
        method: 'Runtime.evaluate',
        params: {
          expression: `(() => ({
            title: document.title,
            url: location.href,
            readyState: document.readyState,
            rootChildren: document.getElementById('root')?.childElementCount || 0,
            homeHeader: Boolean(document.querySelector('.ml-home-header')),
            phoneList: Boolean(document.querySelector('.ml-device-card')),
            mirrorControls: Boolean(document.querySelector('.ml-mirror-card')),
            visibleText: (document.getElementById('root')?.textContent || '').slice(0, 250),
            fallback: Boolean(document.getElementById('startup-fallback')),
          }))()`,
          returnByValue: true,
        },
      }));
    });
    ws.addEventListener('message', ({ data }) => {
      try {
        const result = JSON.parse(String(data));
        if (result.id !== 1) return;
        clearTimeout(t);
        ws.close();
        if (result.error || result.result?.exceptionDetails) {
          reject(Error(JSON.stringify(result.error || result.result.exceptionDetails)));
        } else {
          resolve(result.result?.result?.value);
        }
      } catch (err) {
        clearTimeout(t);
        ws.close();
        reject(err);
      }
    });
    ws.addEventListener('error', (event) => {
      clearTimeout(t);
      reject(Error('WebView2 debug WebSocket error: ' + (event.message || 'connection failed')));
    });
    ws.addEventListener('close', () => clearTimeout(t));
  });
}

let last = 'Waiting for WebView2 CDP endpoint…';
let attempts = 0;
while (Date.now() < deadline) {
  attempts += 1;
  try {
    const response = await fetch(endpoint + '/json/list', {
      signal: AbortSignal.timeout(2500),
    });
    if (!response.ok) throw Error('DevTools status ' + response.status);
    const targets = await response.json();
    for (const target of targets) {
      if (target.type !== 'page' || !target.webSocketDebuggerUrl) continue;
      try {
        const state = await evaluate(target);
        if (state) {
          last = JSON.stringify(state);
          if (state.homeHeader && state.phoneList && state.mirrorControls &&
              state.rootChildren > 0 && state.visibleText?.includes('Mirro')) {
            console.log('PASS: Packaged Windows MirroLeap GUI rendered the real homepage.');
            console.log('WebView2 DOM: ' + last.slice(0, 500));
            process.exit(0);
          }
        }
      } catch (err) {
        last = String(err);
      }
    }
  } catch (err) {
    last = String(err);
  }
  await pause(900);
}
console.error('FAIL: Packaged MirroLeap did not render an interactive homepage within '
  + durationMs / 1000 + ' seconds. Last WebView2 observation: ' + last.slice(0, 1100)
  + '. Attempts: ' + attempts);
process.exit(1);
