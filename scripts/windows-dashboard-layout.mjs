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
    mode === 'running' ? '.ml-stop-action' : '.ml-primary-action');
  await button.waitFor({state: 'visible'});
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
} finally {
  await browser?.close();
  vite.kill('SIGTERM');
}
