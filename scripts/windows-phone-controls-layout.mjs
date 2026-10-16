#!/usr/bin/env node
/**
 * Verify the Windows hover toolbar with actual Chromium/Edge box geometry:
 * all nine quick actions must be visible, unobstructed and never crop the
 * neighboring close/minimize, ink and emergency reset controls. Browser-only
 * mock is inert; native IPC is verified separately by the Rust/Windows build.
 */
import assert from 'node:assert/strict';
import { resolve } from 'node:path';
import { pathToFileURL } from 'node:url';
import { chromium } from 'playwright-core';

const layouts = [
  { layout:'side-right', width:268, height:322 },
  { layout:'side-left', width:268, height:322 },
  { layout:'stack-right', width:168, height:538 },
  { layout:'stack-left', width:168, height:538 },
  { layout:'short-ink-right', width:374, height:212 },
  { layout:'short-ink-left', width:374, height:212 },
  { layout:'tight-ink-right', width:374, height:136 },
  { layout:'tight-ink-left', width:374, height:136 },
];
const file = pathToFileURL(resolve('public/phone-controls.html')).href;
const names = ['fullscreen','home','back','recents','power','rotate','paste','screen_off','top'];
const browser = await chromium.launch({
  channel: process.env.MIRROLEAP_LAYOUT_BROWSER === 'chromium' ? undefined : 'msedge',
  headless: true, args:['--no-first-run', '--disable-extensions'],
});
try {
  for (const dpi of [1, 1.25, 1.5]) {
    const context = await browser.newContext({deviceScaleFactor:dpi});
    for (const entry of layouts) {
      const page = await context.newPage();
      await page.setViewportSize({width:entry.width,height:entry.height});
      const errors = [];
      page.on('pageerror', e => errors.push(e.message));
      await page.addInitScript(() => {
        let open = false;
        let ink = false;
        window.__TAURI__ = {core:{invoke:async (_, {action}) => {
          if (action === 'ink_status') return ink;
          if (action === 'quick_status') return open;
          if (action === 'quick_toggle') {open=!open; ink=false; return open;}
          if (action === 'ink_toggle') {ink=!ink; open=false; return ink;}
          return true;
        }}};
      });
      await page.goto(file);
      await page.evaluate(layout => window.scrcpyControls.setLayout(layout), entry.layout);
      await page.locator('#quick-toggle').click();
      await page.waitForFunction(() => document.body.classList.contains('quick-open'));

      const geometry = await page.evaluate((actions) => {
        const panel = document.getElementById('quick-tools');
        const rail = document.querySelector('.rail');
        const rect = el => el.getBoundingClientRect();
        const r = rect(panel), sidebar = rect(rail);
        const fits = r.left >= -1 && r.top >= -1 &&
          r.right <= window.innerWidth + 1 && r.bottom <= window.innerHeight + 1;
        const noRailOverlap = r.right <= sidebar.left + 1 ||
          sidebar.right <= r.left + 1 || sidebar.bottom <= r.top + 1;
        return {
          fits, noRailOverlap,
          overflowing:panel.scrollWidth - panel.clientWidth > 1 ||
            panel.scrollHeight - panel.clientHeight > 1,
          actions:actions.map(name => {
            const button=document.getElementById('quick-'+name);
            const b=rect(button);
            const x=(b.left+b.right)/2, y=(b.top+b.bottom)/2;
            const hit=document.elementFromPoint(x,y);
            return {name,rect:{x:b.x,y:b.y,w:b.width,h:b.height},hit:hit?.id,visible: b.width>24 && b.height>=34 &&
              b.left>=r.left-1 && b.right<=r.right+1 &&
              b.top>=r.top-1 && b.bottom<=r.bottom+1 &&
              !!hit && (hit === button || button.contains(hit))};
          }),
        };
      }, names);
      assert(geometry.fits,
        `Quick panel clipped in ${entry.layout} at ${dpi * 100}% DPI`);
      assert(geometry.noRailOverlap,
        `Quick panel covers window/ink controls in ${entry.layout}`);
      assert(!geometry.overflowing,
        `Quick panel hides overflowing controls in ${entry.layout}`);
      for(const action of geometry.actions) {
        assert(action.visible,
          `${entry.layout} at ${dpi*100}% DPI: ${action.name} is cropped or obscured: ${JSON.stringify(geometry)}`);
      }
      assert.deepEqual(errors, [], `Browser errors in ${entry.layout}`);
      console.log('PASS',entry.layout, `${dpi*100}% DPI, nine controls reachable`);
      await page.close();
    }
    await context.close();
  }
} finally {
  await browser.close();
}
