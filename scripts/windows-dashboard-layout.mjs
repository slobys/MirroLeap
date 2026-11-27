#!/usr/bin/env node
/**
 * Real Edge/Chromium layout regression. jsdom and React unit tests have no
 * layout engine, so they missed the Start/Stop CTA protruding below its card
 * and overlapping the bottom "More features & shortcuts" link in v4.6.4.
 *
 * Windows CI has Microsoft Edge installed. playwright-core has no bundled
 * browser download; launch the actual Edge with the real production CSS and
 * the dev-only React fixture running against Vite at 127.0.0.1.
 */
import { chromium } from 'playwright-core';
import { spawn } from 'node:child_process';
import assert from 'node:assert/strict';
import { setTimeout as sleep } from 'node:timers/promises';

const origin = 'http://127.0.0.1:1420';
const vite = spawn(process.execPath,
  ['node_modules/vite/bin/vite.js', 'dev', '--host', '127.0.0.1', '--port', '1420', '--strictPort'],
  { stdio: ['ignore', 'pipe', 'pipe'], shell: false });
let viteOutput = '';
vite.stdout.on('data', (chunk) => { viteOutput += chunk.toString().slice(-1000); });
vite.stderr.on('data', (chunk) => { viteOutput += chunk.toString().slice(-1000); });
let browser;

async function serverReady() {
  for (let i = 0; i < 50; i++) {
    if (vite.exitCode !== null) throw Error('Vite server failed: ' + viteOutput);
    try {
      const response = await fetch(origin + '/tests/layout/index.html', {
        signal: AbortSignal.timeout(1500),
      });
      if (response.ok) return;
    } catch { /* server not yet ready */ }
    await sleep(250);
  }
  throw Error('Vite fixture not reachable: ' + viteOutput);
}

async function check(page, mode, viewport, scale) {
  const { width, height } = viewport;
  await page.setViewportSize({ width, height });
  await page.goto(origin + '/tests/layout/index.html?mode=' + mode, {
    waitUntil: 'domcontentloaded',
  });
  await page.locator('.ml-mirror-card').waitFor({state: 'visible'});
  const button = page.locator(
    mode === 'running' || mode === 'switching'
      ? '.ml-stop-action' : '.ml-primary-action');
  await button.waitFor({state: 'visible'});
  // v5.0.19: only ONE quality selector. The dynamic description must remain
  // directly below it at every Windows DPI/viewport size, including while
  // auto-restarting the video or when scrcpy isn't installed yet.
  assert.equal(await page.locator('.ml-mirror-card select').count(),1,
    'Home must not present duplicate scene and resolution dropdowns');
  assert.equal(await page.locator('#ml-preset').count(),0,
    'Legacy scene dropdown was reintroduced');
  const resolution = page.locator('#ml-resolution');
  const summary = page.locator('[data-testid="home-video-summary"]');
  await summary.waitFor({state:'visible'});
  const text = await summary.textContent();
  assert(text?.includes('当前参数：') && text.includes('8Mbps'),
    'Actual size / bitrate / FPS / codec summary is missing: ' + text);
  const selectorBounds = await resolution.boundingBox();
  const summaryBounds = await summary.boundingBox();
  assert(selectorBounds && summaryBounds &&
    summaryBounds.y >= selectorBounds.y + selectorBounds.height - 1,
    'Quality parameter explanation must sit immediately below resolution select');
  if (mode === 'switching') {
    assert(await button.isDisabled(),
      'During an automatic video switch the Stop control must be disabled');
    assert(await button.textContent().then(text => text?.includes('正在切换画质')),
      'During switching the action should say video quality is applying');
    assert.equal(await page.locator('.ml-primary-action').count(),0,
      'Do not offer Start while the existing video stream is restarting');
  }
  const result = await page.evaluate(() => {
    const get = (selector) => {
      const element = document.querySelector(selector);
      if (!element) throw Error('Missing ' + selector);
      const rect = element.getBoundingClientRect();
      return {
        left: rect.left, right: rect.right,
        top: rect.top, bottom: rect.bottom,
        width: rect.width, height: rect.height,
      };
    };
    const scroll = document.querySelector('.ml-home');
    return {
      card: get('.ml-mirror-card'),
      button: get('.ml-mirror-card .ml-primary-action, .ml-mirror-card .ml-stop-action'),
      footer: get('.ml-home-footer'),
      footerLink: get('.ml-footer-link'),
      leftCard: get('.ml-device-card'),
      root: get('.ml-home'),
      scrollHeight: scroll.scrollHeight,
      clientHeight: scroll.clientHeight,
    };
  });
  const { card, button: btn, footer, footerLink, leftCard, scrollHeight, clientHeight } = result;
  const desc = [mode, width + 'x' + height, 'scale=' + scale].join(' ');
  // A CTA must fit *inside* the right-hand card with its original round
  // corners/padding, including the extra "next session" note when running.
  assert(btn.top >= card.top + 10, desc + ': CTA above card');
  assert(btn.bottom <= card.bottom - 10, desc + ': CTA below card: ' + JSON.stringify(result));
  assert(btn.left >= card.left + 10, desc + ': CTA crosses left card border');
  assert(btn.right <= card.right - 10, desc + ': CTA crosses right card border');
  // The persistent footer link may be BELOW THE VIEWPORT on short screens,
  // but it must never be painted on top of the CTA or inside the card.
  const endOfGrid = Math.max(leftCard.bottom, card.bottom);
  assert(footer.top >= endOfGrid + 8,
    desc + ': Footer overlaps either card: ' + JSON.stringify(result));
  assert(footerLink.top >= footer.top, desc + ': footer link protrudes above footer');
  assert(scrollHeight + 2 >= clientHeight, desc + ': invalid scroll container');

  // When content is taller than the viewport, scroll the page (not the
  // nested card) to reach the CTA, then the bottom footer link.
  await button.scrollIntoViewIfNeeded({timeout: 5000});
  assert(await button.isVisible(), desc + ': CTA not visible after scrolling');
  await page.locator('.ml-footer-link').scrollIntoViewIfNeeded({timeout: 5000});
  const visibleLink = await page.locator('.ml-footer-link').boundingBox();
  assert(visibleLink && visibleLink.y >= -2 &&
    visibleLink.y + visibleLink.height <= height + 2,
    desc + ': footer not reachable after scrolling: ' + JSON.stringify(visibleLink));
  // Hit-test at the visual center: the CTA must not cover the footer.
  const hit = await page.evaluate(() => {
    const el = document.querySelector('.ml-footer-link');
    const r = el.getBoundingClientRect();
    const touched = document.elementFromPoint(r.left + r.width / 2, r.top + r.height / 2);
    return touched === el || el.contains(touched);
  });
  assert(hit, desc + ': footer visually obstructed');
  console.log('PASS', desc, 'CTA inside card / footer separate / both reachable',
    scrollHeight > clientHeight
      ? `(scroll ${Math.ceil(scrollHeight - clientHeight)}px)`
      : '(fits window)');
}

async function checkAirPlayWaiting(browser, scale) {
  const context = await browser.newContext({
    viewport: {width: 900, height: 680}, deviceScaleFactor: scale,
  });
  try {
    await context.addInitScript(() => {
      const status = {
        supported: true, installed: true, running: false, installing: false,
        receiverReady: false, receiverName: 'AirPlay-Windows', receiverIp: null,
        usbStatus: 'ready', windowStyle: 'device-auto',
        diagnostics: {usbVideo:'idle', bluetooth:'off'},
      };
      window.addEventListener('fixture-wifi-frame', () => {
        Object.assign(status, {sourceWidth: 1246, sourceHeight: 1792});
      });
      window.__TAURI_INTERNALS__ = {
        invoke: async (command, args) => {
          if (command === 'ios_airplay_start') {
            Object.assign(status, {running: true, receiverReady: true, receiverIp: '192.0.2.107'});
          } else if (command === 'ios_airplay_stop') {
            Object.assign(status, {running: false, receiverReady: false, receiverIp: null});
          } else if (command === 'ios_usb_control') {
            status.usbStatus = args.start ? 'streaming' : 'ready';
            status.diagnostics = args.start ? {usbVideo:'paused', bluetooth:'ready', bluetoothTransport:'usb'} :
              {usbVideo:'idle', bluetooth:'off'};
          } else if (command === 'ios_airplay_export_diagnostics') {
            return 'mirroleap-ios-layout-test.json';
          } else if (command === 'ios_airplay_set_receiver_name') {
            status.receiverName = args.name;
          }
          return {...status};
        },
      };
    });
    const page = await context.newPage();
    await page.goto(origin + '/tests/layout/index.html');
    const panel = page.getByTestId('ios-home-panel');
    const connection = panel.getByTestId('ios-connection-info');
    const usb = panel.getByText('USB已连接', {exact: true});
    const toggle = panel.getByRole('button', {name: '投屏详情'});
    const details = panel.locator('#ios-receiver-details');
    await usb.waitFor({state: 'visible'});
    assert.equal(await toggle.getAttribute('aria-expanded'), 'false');
    assert(await details.isHidden());
    const usbBounds = await usb.boundingBox();
    const toggleBounds = await toggle.boundingBox();
    const titleBounds = await panel.getByRole('heading', {name: 'iPhone / iPad 投屏'}).boundingBox();
    const sizeBounds = await panel.getByRole('combobox', {name: 'iOS 投屏窗口大小'}).boundingBox();
    assert(usbBounds && toggleBounds && titleBounds && sizeBounds &&
      usbBounds.x >= titleBounds.x + titleBounds.width &&
      usbBounds.y < sizeBounds.y && toggleBounds.y >= sizeBounds.y + sizeBounds.height,
    'USB status should sit beside the title and the details button below the settings');
    await toggle.click();
    assert(await details.isVisible());
    await toggle.click();
    assert(await details.isHidden() && await usb.isVisible());
    await panel.getByRole('button', {name: '启动 iOS 投屏'}).click();
    await connection.getByText('AirPlay-Windows', {exact: true}).waitFor();
    await connection.getByText('192.0.2.107', {exact: true}).waitFor();
    await connection.getByRole('button', {name: '编辑接收器名称'}).click();
    await connection.getByRole('textbox', {name: '接收器名称'}).fill('书房 iPad');
    await connection.getByRole('button', {name: '保存'}).click();
    await connection.getByText('书房 iPad', {exact: true}).waitFor();
    assert(await panel.getByText('接收器已启动 · 等待手机连接').isVisible());
    assert.equal(await panel.getByRole('button', {name: '全屏'}).count(), 0);
    const panelRect = await panel.boundingBox();
    const infoRect = await connection.boundingBox();
    assert(panelRect && infoRect && infoRect.x >= panelRect.x &&
      infoRect.x + infoRect.width <= panelRect.x + panelRect.width);
    const wifi = panel.getByText('WiFi连接', {exact: true});
    const usbMirroring = panel.getByText('USB连接', {exact: true});
    assert.equal(await wifi.count(), 0);
    await page.evaluate(() => window.dispatchEvent(new Event('fixture-wifi-frame')));
    await wifi.waitFor({state: 'visible'});
    await panel.getByRole('button', {name: 'USB投屏'}).click();
    await usbMirroring.waitFor({state: 'visible'});
    await page.setViewportSize({width: 620, height: 480});
    await toggle.click();
    assert((await details.getByTestId('ios-video-detail').innerText()).includes('暂无新画面'));
    assert((await details.getByTestId('ios-bluetooth-detail').innerText()).includes('键鼠可用'));
    const exportButton = details.getByRole('button', {name:'导出诊断日志'});
    await exportButton.click();
    await details.getByText(/已保存到「下载」文件夹/).waitFor({state:'visible'});
    const layoutFits = await details.evaluate(root => {
      const bounds = root.getBoundingClientRect();
      return [...root.querySelectorAll('dl dd, button')].every(element => {
        const rect = element.getBoundingClientRect();
        return rect.left >= bounds.left - 1 && rect.right <= bounds.right + 1 &&
          parseFloat(getComputedStyle(element).fontSize) >= 11;
      });
    });
    assert(layoutFits,'Details must remain legible and inside the panel at a narrow viewport');
    await toggle.click();
    const narrowSize = await panel.getByRole('combobox', {name: 'iOS 投屏窗口大小'}).boundingBox();
    const narrowWifi = await wifi.boundingBox();
    const narrowUsb = await usbMirroring.boundingBox();
    assert(narrowSize && narrowWifi && narrowUsb &&
      narrowWifi.y + narrowWifi.height < narrowSize.y &&
      narrowUsb.y + narrowUsb.height < narrowSize.y,
    'Both transport labels must remain in the header on narrow windows');
    await panel.getByRole('button', {name: '停止接收'}).click();
    await wifi.waitFor({state: 'detached'});
    assert(await usbMirroring.isVisible());
    console.log('PASS receiver rename, USB/WiFi labels, and collapsed details at scale', scale);
  } finally {
    await context.close();
  }
}

try {
  await serverReady();
  browser = await chromium.launch({
    // GitHub Windows runners include Edge. Linux development can explicitly
    // run the identical fixture with a Playwright-managed Chromium browser.
    channel: process.env.MIRROLEAP_LAYOUT_BROWSER === 'chromium' ? undefined : 'msedge',
    headless: true,
    args: ['--no-first-run', '--disable-extensions'],
  });
  for (const scale of [1, 1.5]) {
    const context = await browser.newContext({
      viewport: {width: 940, height: 680}, deviceScaleFactor: scale,
    });
    const page = await context.newPage();
    for (const mode of ['ready', 'running', 'no-core', 'switching']) {
      for (const viewport of [
        {width: 940, height: 680}, // default Tauri CSS viewport
        {width: 740, height: 550}, // allowed minimum window
        {width: 1300, height: 940}, // maximized, as in user's screenshot
        {width: 620, height: 480}, // responsive one-column layout
      ]) {
        await check(page, mode, viewport, scale);
      }
    }
    await context.close();
  }
  for (const scale of [1, 1.25, 1.5]) {
    await checkAirPlayWaiting(browser, scale);
  }
} finally {
  await browser?.close();
  vite.kill('SIGTERM');
}
