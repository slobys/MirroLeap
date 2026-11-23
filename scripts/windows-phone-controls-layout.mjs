#!/usr/bin/env node
/**
 * Verify the Windows hover toolbar with actual Chromium/Edge box geometry:
 * all quick actions and all seven presenter actions must be visible,
 * unobstructed and never crop the
 * neighboring close/minimize, ink and emergency reset controls. Browser-only
 * mock is inert; native IPC is verified separately by the Rust/Windows build.
 */
import assert from 'node:assert/strict';
import { resolve } from 'node:path';
import { pathToFileURL } from 'node:url';
import { chromium } from 'playwright-core';

const layouts = [
  { layout:'side-right', width:268, height:370 },
  { layout:'side-left', width:268, height:370 },
  { layout:'stack-right', width:168, height:586 },
  { layout:'stack-left', width:168, height:586 },
  { layout:'short-ink-right', width:374, height:212 },
  { layout:'short-ink-left', width:374, height:212 },
  { layout:'tight-ink-right', width:374, height:136 },
  { layout:'tight-ink-left', width:374, height:136 },
];
const file = pathToFileURL(resolve('public/phone-controls.html')).href;
const names = ['fullscreen','home','back','recents','power','rotate','paste','top','remember'];
const presenterActions = ['spotlight','magnifier','live','privacy','creator-click','refresh','off'];
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
        let presenter = false;
        let presentMode = 'off';
        window.__TAURI__ = {core:{invoke:async (_, {action}) => {
          if (action === 'ink_status') return ink;
          if (action === 'present_status') return presenter;
          if (action === 'present_is_spotlight') return presentMode === 'spotlight';
          if (action === 'present_is_magnifier') return presentMode === 'magnifier';
          if (action === 'present_is_live') return presentMode === 'live';
          if (action === 'present_privacy_is_dim' || action === 'present_privacy_is_black') return false;
          if (action === 'present_toggle') {
            presenter = !presenter; open = false; ink = false;
            if (!presenter) presentMode = 'off';
            return presenter;
          }
          if (action === 'present_spotlight') {presentMode = 'spotlight';return true;}
          if (action === 'present_magnifier') {presentMode = 'magnifier';return true;}
          if (action === 'present_refresh') return true;
          if (action === 'present_off') {presentMode = 'off';return false;}
          if (action === 'quick_status') return open;
          if (action === 'quick_toggle') {open=!open; ink=false; presenter=false;return open;}
          if (action === 'ink_toggle') {ink=!ink; open=false; presenter=false;return ink;}
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
      const railControls = await page.locator('.annotation-actions button').count();
      assert.equal(railControls,4,'Presenter button must remain visible on the toolbar');
      await page.locator('#present-toggle').click();
      await page.waitForFunction(() => document.body.classList.contains('present-open'));
      const presenterGeometry = await page.evaluate((actions) => {
        const panel=document.getElementById('present-tools');
        const rail=document.querySelector('.rail');
        const p=panel.getBoundingClientRect(), r=rail.getBoundingClientRect();
        return {
          fits:p.left>=-1 && p.top>=-1 && p.right<=innerWidth+1 && p.bottom<=innerHeight+1,
          overlap:!(p.right<=r.left+1 || p.left>=r.right-1 ||
            p.bottom<=r.top+1 || p.top>=r.bottom-1),
          overflow:panel.scrollWidth-panel.clientWidth>1 || panel.scrollHeight-panel.clientHeight>1,
          controls:actions.map(name=>{
            const el=document.getElementById(name==='creator-click' ? name : 'present-'+name);
            const b=el.getBoundingClientRect();
            const hit=document.elementFromPoint(b.left+b.width/2,b.top+b.height/2);
            const textFits = Number.parseFloat(getComputedStyle(el).fontSize) >= 11 &&
              el.scrollWidth <= el.clientWidth && el.scrollHeight <= el.clientHeight;
            return {name,ok:textFits&&b.width>=34&&b.height>=34&&b.left>=p.left-1&&b.right<=p.right+1&&
              b.top>=p.top-1&&b.bottom<=p.bottom+1&&(hit===el||el.contains(hit))};
          }),
        };
      }, presenterActions);
      assert(presenterGeometry.fits && !presenterGeometry.overlap && !presenterGeometry.overflow,
        `Presenter window clipped: ${entry.layout} ${dpi} ${JSON.stringify(presenterGeometry)}`);
      for(const item of presenterGeometry.controls){
        assert(item.ok,`Presenter ${item.name} obscured: ${entry.layout} ${dpi} ${JSON.stringify(presenterGeometry)}`);
      }
      console.log('PASS',entry.layout,`${dpi*100}% DPI, all quick + seven presenter controls reachable`);
      await page.close();
    }
    for (const usb of [false,true]) {
    const airplay = await context.newPage();
    await airplay.setViewportSize({width:268,height:370});
    await airplay.addInitScript(usb => {
      window.__MIRROLEAP_AIRPLAY__ = true;
      window.__MIRROLEAP_IOS_USB__ = usb;
      let quick = false;
      let present = false;
      let frozen = false;
      let cameraRight = false;
      let remember = true;
      window.__TAURI__ = {core:{invoke:async (_, {action}) => {
        if (action === 'quick_toggle') { quick = !quick; present = false; }
        if (action === 'present_toggle') { present = !present; quick = false; }
        if (action === 'quick_toggle' || action === 'quick_status') return quick;
        if (action === 'present_toggle' || action === 'present_status') return present;
        if (action === 'present_magnifier') { frozen = !frozen; return frozen; }
        if (action === 'present_is_magnifier') return frozen;
        if (action === 'quick_camera_edge') { cameraRight = !cameraRight; return cameraRight; }
        if (action === 'quick_remember_toggle') remember = !remember;
        if (action === 'quick_remember_toggle' || action === 'quick_remember_status') return remember;
        if (action === 'present_refresh') { window.refreshed = true; return true; }
        return false;
      }}};
    }, usb);
    await airplay.goto(file);
    await airplay.evaluate(() => window.scrcpyControls.setLayout('side-right'));
    await airplay.locator('#quick-toggle').click();
    await airplay.waitForFunction(() => document.body.classList.contains('quick-open'));
    for (const action of ['fullscreen','top','camera-edge','remember']) {
      assert(await airplay.locator(`#quick-${action}`).isVisible());
    }
    for (const action of ['home','back','recents','power','rotate','paste']) {
      assert(await airplay.locator(`#quick-${action}`).isHidden());
    }
    assert(await airplay.locator('#quick-record').count() === 0);
    const remember = airplay.locator('#quick-remember');
    for (const enabled of ['false','true']) {
      await remember.click();
      await airplay.waitForFunction(pressed =>
        document.getElementById('quick-remember').getAttribute('aria-pressed') === pressed, enabled);
    }
    for (const edge of ['右','左']) {
      await airplay.getByRole('button',{name:'切换横屏摄像头位置'}).click();
      await airplay.getByText(`横屏摄像头已设为${edge}侧`,{exact:true}).waitFor();
    }
    await airplay.locator('#present-toggle').click();
    await airplay.waitForFunction(() => document.body.classList.contains('present-open'));
    assert(await airplay.locator('#present-live').isVisible());
    assert(await airplay.locator('#present-spotlight').isVisible());
    await airplay.locator('#present-magnifier').click();
    await airplay.locator('#present-refresh').click();
    assert(await airplay.evaluate(() => window.refreshed === true));
    assert(await airplay.locator('#present-privacy').isEnabled());
    const airplayLabelsFit = await airplay.locator('.present-grid button:visible').evaluateAll(buttons =>
      buttons.every(button => Number.parseFloat(getComputedStyle(button).fontSize) >= 11 &&
        button.scrollWidth <= button.clientWidth && button.scrollHeight <= button.clientHeight));
    assert(airplayLabelsFit,'AirPlay presentation labels must be readable and fit their buttons');
    console.log(`PASS ${usb?'USB':'AirPlay'} quick and presentation tools`,`${dpi*100}% DPI`);
    await airplay.close();
    }
    await context.close();
  }
} finally {
  await browser.close();
}
