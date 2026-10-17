#!/usr/bin/env node
/**
 * Browser geometry gate for the actual MirroLeap settings React components.
 * Catch tiny labels, clipped controls and unreachable scroll content at
 * Windows window sizes / DPI settings (jsdom cannot calculate box geometry).
 */
import assert from 'node:assert/strict';
import { spawn } from 'node:child_process';
import { setTimeout as sleep } from 'node:timers/promises';
import { chromium } from 'playwright-core';

const base = 'http://127.0.0.1:1420';
const vite = spawn(process.execPath, [
  'node_modules/vite/bin/vite.js', 'dev', '--host', '127.0.0.1',
  '--port', '1420', '--strictPort',
], {stdio: ['ignore', 'pipe', 'pipe'], shell:false});
let viteOutput = '';
vite.stdout.on('data', chunk => {viteOutput += chunk.toString().slice(-600);});
vite.stderr.on('data', chunk => {viteOutput += chunk.toString().slice(-600);});
let browser;

async function waitForVite() {
  for (let i = 0; i < 60; i++) {
    if (vite.exitCode !== null) throw Error('Vite exited: ' + viteOutput);
    try {
      const r = await fetch(base + '/tests/layout/settings.html', {
        signal: AbortSignal.timeout(1200),
      });
      if (r.ok) return;
    } catch { /* wait for ready */ }
    await sleep(250);
  }
  throw Error('Settings fixture not reachable: ' + viteOutput);
}

async function verify(page, viewport, scale) {
  await page.setViewportSize(viewport);
  // The visual fixture runs in Edge/Chromium without a Tauri host. Mock
  // only the inert event bridge needed to mount presenter controls; actual
  // Windows IPC and WebView2 startup are checked in separate CI gates.
  await page.addInitScript(() => {
    let callback = 0;
    Object.defineProperty(window, '__TAURI_INTERNALS__', {
      configurable: true,
      value: {
        transformCallback: () => ++callback,
        invoke: async () => 1,
      },
    });
    Object.defineProperty(window, '__TAURI_EVENT_PLUGIN_INTERNALS__', {
      configurable: true,
      value: {unregisterListener: () => {}},
    });
  });
  const problems = [];
  page.on('pageerror', e => problems.push(e.message));
  await page.goto(base + '/tests/layout/settings.html', {waitUntil:'domcontentloaded'});
  await page.locator('.ml-comfort-settings').waitFor({state:'visible'});
  const geometry = await page.evaluate(() => {
    const find = (selector) => {
      const node = document.querySelector(selector);
      if (!node) throw Error('Missing element ' + selector);
      const r = node.getBoundingClientRect();
      return {left:r.left,right:r.right,top:r.top,bottom:r.bottom,
        width:r.width,height:r.height};
    };
    const el = (selector) => {
      const node = document.querySelector(selector);
      if (!node) throw Error('Missing element ' + selector);
      return node;
    };
    const container = el('.ml-settings-body');
    const grid = el('.ml-comfort-settings .grid.grid-cols-1');
    const outer = el('.ml-mirror-layout');
    const label = el('.ml-comfort-settings label');
    return {
      screen: {width:document.documentElement.scrollWidth,
        height:document.documentElement.scrollHeight},
      nav:find('.ml-settings-nav'), panel:find('.ml-settings-body'),
      grid:find('.ml-comfort-settings .grid.grid-cols-1'),
      control:find('.ml-comfort-settings button'),
      labelFont:parseFloat(getComputedStyle(label).fontSize),
      gridColumns:getComputedStyle(grid).gridTemplateColumns.split(' ').length,
      outerColumns:(getComputedStyle(outer).gridTemplateColumns.match(/minmax\([^)]*\)|\S+/g) || []).length,
      outerTemplate:getComputedStyle(outer).gridTemplateColumns,
      scrollHeight:container.scrollHeight, clientHeight:container.clientHeight,
      overflowY:getComputedStyle(container).overflowY,
      horizontalOverflow:container.scrollWidth - container.clientWidth,
    };
  });
  const desc = `${viewport.width}x${viewport.height} / ${scale * 100}% DPI`;
  assert(geometry.labelFont >= 14, desc + ': small text label ' + geometry.labelFont);
  assert(geometry.control.height >= 42, desc + ': small click target');
  assert(geometry.panel.width >= 250 && geometry.panel.height >= 200,
    desc + ': collapsed settings body');
  assert(geometry.overflowY === 'auto', desc + ': scrolling disabled');
  assert(geometry.screen.width <= viewport.width + 3,
    desc + ': document horizontally overflows: ' + JSON.stringify(geometry));
  assert(geometry.horizontalOverflow <= 3,
    desc + ': settings content horizontally clipped: ' + JSON.stringify(geometry));
  const expectedColumns = viewport.width < 640 ? 1 : 2;
  assert.equal(geometry.gridColumns, expectedColumns,
    desc + ': incorrect capture-settings column count');
  assert.equal(geometry.outerColumns, viewport.width > 1050 ? 2 : 1,
    desc + ': mirrored layout did not adapt to available width: ' + geometry.outerTemplate);

  // The bottom action must remain reachable at the minimum window size,
  // even though it is intentionally below the fold instead of being squeezed.
  const action = page.locator('.ml-comfort-settings > div:last-child > button');
  await action.scrollIntoViewIfNeeded({timeout:5000});
  assert(await action.isVisible(), desc + ': bottom control inaccessible');
  const hitBottom = await action.evaluate(el => {
    const rect = el.getBoundingClientRect();
    const touched = document.elementFromPoint(
      rect.left + rect.width / 2, rect.top + rect.height / 2);
    return touched === el || el.contains(touched);
  });
  assert(hitBottom, desc + ': last control is obscured');

  const details = page.locator('.ml-settings-details');
  const summary = details.locator('summary');
  await summary.click({timeout:5000});
  assert(await details.evaluate(el => el.open), desc + ': advanced section cannot expand');
  await summary.click();
  assert(!(await details.evaluate(el => el.open)), desc + ': advanced section cannot collapse');

  // The sticky page-level navigation is horizontally scrollable; every
  // category remains reachable in a 740px window without shrinking text.
  const lastTab = page.locator('.ml-settings-nav button').last();
  await lastTab.scrollIntoViewIfNeeded({timeout:5000});
  const navHit = await lastTab.evaluate(el => {
    const r = el.getBoundingClientRect();
    const hit = document.elementFromPoint(r.left + r.width / 2, r.top + r.height / 2);
    return hit === el || el.contains(hit);
  });
  assert(navHit, desc + ': settings navigation is obstructed');

  const behaviorTab = page.locator('.ml-settings-nav button').nth(2);
  await behaviorTab.click();
  await page.locator('.ml-behavior-groups').waitFor({state:'visible'});
  const behavior = await page.evaluate(() => {
    const grid = document.querySelector('.ml-behavior-groups');
    if (!grid) throw Error('Behavior grid missing');
    return {
      columns:(getComputedStyle(grid).gridTemplateColumns.match(/minmax\([^)]*\)|\S+/g) || []).length,
      scrollHeight:document.querySelector('.ml-settings-body').scrollHeight,
      clientHeight:document.querySelector('.ml-settings-body').clientHeight,
    };
  });
  assert.equal(behavior.columns, viewport.width <= 800 ? 1 : 2,
    desc + ': behavior section overpacked');
  assert.equal(await page.locator('.ml-settings-group').count(), 2,
    desc + ': only device/connection and recording sections belong in behavior settings');
  assert.equal(await page.locator('.ml-settings-group-head').allTextContents()
    .then(titles => titles.some(title=>title.includes('窗口与显示') || title.includes('Window & display'))),false,
    desc + ': duplicate window settings should remain on home or quick toolbar');

  const appearanceTab = page.locator('.ml-settings-nav button').nth(3);
  await appearanceTab.click();
  await page.locator('.ml-appearance-controls').waitFor({state:'visible'});
  assert.equal(await page.locator('.ml-theme-swatch').count(), 5,
    desc + ': theme controls disappeared');
  const appearanceColumns = await page.locator('.ml-appearance-layout')
    .evaluate(el => (getComputedStyle(el).gridTemplateColumns.match(/minmax\([^)]*\)|\S+/g) || []).length);
  assert.equal(appearanceColumns, viewport.width > 1050 ? 2 : 1,
    desc + ': appearance layout is not responsive');

  const toolsTab = page.locator('.ml-settings-nav button').nth(4);
  await toolsTab.click();
  await page.locator('.ml-tools-layout').waitFor({state:'visible'});
  assert.equal(await page.locator('.ml-tools-layout .ml-card').count(), 2,
    desc + ': presenter and capture cards missing');
  const toolColumns = await page.locator('.ml-tools-layout')
    .evaluate(el => (getComputedStyle(el).gridTemplateColumns.match(/minmax\([^)]*\)|\S+/g) || []).length);
  assert.equal(toolColumns, viewport.width > 1050 ? 2 : 1,
    desc + ': tools layout is not responsive');
  const contentWidth = await page.locator('.ml-settings-body')
    .evaluate(el => el.scrollWidth - el.clientWidth);
  assert(contentWidth <= 3, desc + ': tools horizontally overflow by ' + contentWidth);
  assert(await page.locator('.ml-settings-help-details').count() >= 2,
    desc + ': compact tools lost safety explanations');
  assert.deepEqual(problems, [], desc + ': React layout errors');
  console.log('PASS', desc,
    'mirror / grouped behavior / appearance / compact tools / scroll');
}

try {
  await waitForVite();
  browser = await chromium.launch({
    channel: process.env.MIRROLEAP_LAYOUT_BROWSER === 'chromium' ? undefined : 'msedge',
    headless:true, args:['--no-first-run','--disable-extensions'],
  });
  for (const scale of [1,1.25,1.5]) {
    const context = await browser.newContext({deviceScaleFactor:scale});
    const page = await context.newPage();
    for (const viewport of [
      {width:940,height:680}, {width:740,height:550},
      {width:1300,height:940}, {width:620,height:480},
      {width:1080,height:680}, {width:1420,height:1024},
    ]) await verify(page, viewport, scale);
    await context.close();
  }
} finally {
  await browser?.close();
  vite.kill('SIGTERM');
}
