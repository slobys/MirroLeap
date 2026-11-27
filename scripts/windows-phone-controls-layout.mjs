#!/usr/bin/env node
/**
 * Verify the Windows hover toolbar with actual Chromium/Edge box geometry:
 * all quick actions and all seven presenter actions must be visible,
 * unobstructed and never crop the
 * neighboring close/minimize, ink and emergency reset controls. Browser-only
 * mock is inert; native IPC is verified separately by the Rust/Windows build.
 */
import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
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
const inputScript = await readFile(resolve('public/ios-input.js'), 'utf8');
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
      let bluetooth = {phase:'off'};
      window.iosKeysSent = [];
      window.__TAURI__ = {core:{invoke:async (command, {action,token}) => {
        if (command === 'device_frame_ios_bluetooth') {
          if (action === 'start') bluetooth = {phase:'confirm',deviceName:'验收 iPad',token:42};
          if (action === 'confirm' && token === 42) {
            bluetooth = {phase:'connected',deviceName:'验收 iPad',pointerReady:true};
          }
          if (action === 'stop') bluetooth = {phase:'off'};
          return bluetooth;
        }
        if (['quick_home','quick_back','quick_recents','quick_power'].includes(action)) {
          assertConnected();
          window.iosKeysSent.push(action);
          return true;
        }
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
      function assertConnected() {
        if (bluetooth.phase !== 'connected') throw new Error('Bluetooth target was not confirmed');
      }
    }, usb);
    await airplay.goto(file);
    await airplay.evaluate(() => window.scrcpyControls.setLayout('side-right'));
    await airplay.locator('#quick-toggle').click();
    await airplay.waitForFunction(() => document.body.classList.contains('quick-open'));
    for (const action of ['fullscreen','top','camera-edge','remember']) {
      assert(await airplay.locator(`#quick-${action}`).isVisible());
    }
    for (const action of ['rotate','paste']) {
      assert(await airplay.locator(`#quick-${action}`).isHidden());
    }
    const keys = ['home','back','recents','power'];
    for (const action of keys) {
      assert(await airplay.locator(`#quick-${action}`).isVisible());
      assert(await airplay.locator(`#quick-${action}`).isDisabled());
    }
    const bluetooth = airplay.locator('#quick-ios-bluetooth');
    await bluetooth.click();
    await bluetooth.getByText('确认设备').waitFor();
    await bluetooth.click();
    await bluetooth.getByText('断开蓝牙').waitFor();
    await airplay.getByText('蓝牙已连接 · 可直接操作画面').waitFor();
    assert.equal(await airplay.locator('#quick-input').count(), 0);
    for (const entry of layouts) {
      await airplay.setViewportSize({width:entry.width,height:entry.height});
      await airplay.evaluate(layout => window.scrcpyControls.setLayout(layout), entry.layout);
      const clipped = await airplay.locator('.quick-grid button:visible').evaluateAll(buttons => {
        const panel = document.getElementById('quick-tools').getBoundingClientRect();
        return buttons.filter(button => {
          const rect = button.getBoundingClientRect();
          return !(rect.left >= panel.left && rect.right <= panel.right &&
            rect.top >= panel.top && rect.bottom <= panel.bottom &&
            button.scrollWidth <= button.clientWidth && button.scrollHeight <= button.clientHeight);
        }).map(button => ({
          id:button.id, rect:button.getBoundingClientRect().toJSON(),
          panel:panel.toJSON(), width:[button.clientWidth,button.scrollWidth],
          height:[button.clientHeight,button.scrollHeight],
        }));
      });
      assert.deepEqual(clipped, [], `iOS quick menu clipped at ${entry.layout} ${dpi * 100}%`);
    }
    await airplay.setViewportSize({width:268,height:370});
    await airplay.evaluate(() => window.scrcpyControls.setLayout('side-right'));
    for (const action of keys) await airplay.locator(`#quick-${action}`).click();
    assert.deepEqual(await airplay.evaluate(() => window.iosKeysSent), keys.map(action => `quick_${action}`));
    await bluetooth.click();
    await bluetooth.getByText('连接蓝牙').waitFor();
    for (const action of keys) assert(await airplay.locator(`#quick-${action}`).isDisabled());
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
    const inputPage = await context.newPage();
    await inputPage.setViewportSize({width:600,height:500});
    await inputPage.setContent('<div tabindex="-1" id="ios-input" style="position:absolute;left:100px;top:40px;width:400px;height:400px"></div><input id="pc" style="position:absolute;top:450px">');
    await inputPage.evaluate(() => {
      window.__MIRROLEAP_AIRPLAY__ = true;
      window.sent = [];
      window.__TAURI__ = {core:{invoke:async (command, {events}) => {
        if (command !== 'device_frame_ios_input') return;
        window.sent.push(...events);
        if (window.holdClick && events.some(report => report.kind === 'click')) {
          await new Promise(resolve => { window.finishClick = resolve; });
        }
      }}};
    });
    await inputPage.addScriptTag({content:inputScript});
    await inputPage.mouse.click(200, 140);
    assert.deepEqual(await inputPage.evaluate(() => window.sent), []);
    await inputPage.evaluate(() => window.mirroleapIosInput.setEnabled(true));
    await inputPage.mouse.move(200, 140);
    await inputPage.keyboard.press('b');
    await inputPage.mouse.down();
    await inputPage.mouse.move(207, 142);
    assert.equal(await inputPage.evaluate(() => window.sent.some(report => report.buttons > 0)), false);
    await inputPage.mouse.up();
    await inputPage.waitForFunction(() => window.sent.some(report => report.kind === 'click'));
    await inputPage.mouse.move(200, 140);
    await inputPage.keyboard.down('Alt');
    await inputPage.mouse.down();
    await inputPage.mouse.move(300, 240);
    await inputPage.mouse.up();
    await inputPage.keyboard.up('Alt');
    await inputPage.keyboard.type('a');
    await inputPage.mouse.wheel(0, 100);
    await inputPage.waitForFunction(() => window.sent.some(report => report.wheel > 0));
    await inputPage.mouse.wheel(0, -100);
    await inputPage.waitForFunction(() => window.sent.some(report => report.wheel < 0));
    await inputPage.keyboard.down('Shift');
    await inputPage.mouse.down();
    await inputPage.mouse.move(20, 460);
    await inputPage.waitForFunction(() => window.sent.at(-1)?.kind === 'release');
    await inputPage.mouse.up();
    await inputPage.keyboard.up('Shift');
    await inputPage.locator('#pc').click();
    await inputPage.keyboard.type('pc');
    assert.equal(await inputPage.locator('#pc').inputValue(), 'pc');
    const sent = await inputPage.evaluate(() => window.sent);
    assert.deepEqual(sent.filter(report => report.kind === 'click'),
      [{kind:'click', x:2500, y:2500, button:1}], 'A stationary click must dispatch once on mouse-up');
    assert(sent.some(report => report.kind === 'pointer' && report.buttons === 1 &&
      report.x === 2500 && report.y === 2500), 'Drag must start at the original press position');
    assert(sent.some(report => report.kind === 'pointer' && report.buttons === 0 &&
      report.x === 5000 && report.y === 5000), 'Release must follow the dragged point');
    assert.deepEqual(sent.filter(report => report.kind === 'pointer' && report.wheel).map(report =>
      report.wheel), [1, -1], 'Wheel down and up must match the iPad scroll direction');
    assert.deepEqual(sent.filter(report => report.kind === 'key').map(report =>
      [report.usage,report.down]), [[4,true],[4,false],[0xe1,true]],
      'Alt used for mouse selection must stay on Windows');
    assert.equal(sent.at(-1).kind, 'release', 'Leaving must release the device before PC typing');

    await inputPage.evaluate(() => { window.sent.length = 0; window.holdClick = true; });
    await inputPage.mouse.click(200, 140);
    await inputPage.waitForFunction(() => Boolean(window.finishClick));
    await inputPage.keyboard.type('abba12');
    assert.equal(await inputPage.evaluate(() => window.sent.some(report => report.kind === 'key')), false,
      'Typing must stay queued behind the click that focused the text field');
    await inputPage.evaluate(() => { window.holdClick = false; window.finishClick(); });
    await inputPage.waitForFunction(() => window.sent.filter(report => report.kind === 'key').length === 12);
    const typed = await inputPage.evaluate(() => window.sent.filter(report => report.kind === 'key')
      .map(report => [report.usage,report.down]));
    assert.deepEqual(typed, [4,5,5,4,30,31].flatMap(usage => [[usage,true],[usage,false]]),
      'The first key, rapid letters, and consecutive identical keys must all reach HID in order');

    await inputPage.mouse.move(200, 140);
    await inputPage.evaluate(() => { window.sent.length = 0; });
    await inputPage.keyboard.down('Alt');
    await inputPage.mouse.down();
    await inputPage.mouse.move(250, 170);
    await inputPage.waitForFunction(() => window.sent.some(report => report.buttons === 1));
    await inputPage.keyboard.press('Escape');
    await inputPage.mouse.move(275, 190);
    await inputPage.mouse.up();
    await inputPage.keyboard.up('Alt');
    await inputPage.mouse.click(300, 240);
    await inputPage.waitForFunction(() => window.sent.some(report => report.kind === 'click'));
    const afterEscape = await inputPage.evaluate(() => {
      const escape = window.sent.findIndex(report => report.kind === 'key' && report.usage === 0x29);
      return escape < 0 ? [] : window.sent.slice(escape);
    });
    assert(afterEscape.some(report => report.kind === 'pointer' && report.buttons === 0));
    assert(afterEscape.every(report => !report.buttons), 'Escape must clear the pressed mouse button');
    assert.deepEqual(afterEscape.filter(report => report.kind === 'click'),
      [{kind:'click', x:5000, y:5000, button:1}], 'A new click must work after Escape cancels a drag');

    await inputPage.evaluate(() => { window.sent.length = 0; });
    await inputPage.mouse.down();
    await inputPage.mouse.move(303, 160, {steps:5});
    await inputPage.mouse.up();
    await inputPage.mouse.move(300, 240);
    await inputPage.mouse.down();
    await inputPage.mouse.move(230, 243, {steps:5});
    await inputPage.mouse.up();
    await inputPage.mouse.move(300, 240);
    await inputPage.mouse.down();
    await inputPage.mouse.move(370, 243, {steps:5});
    await inputPage.mouse.up();
    await inputPage.waitForFunction(() => window.sent.some(report => report.kind === 'scroll' && report.pan > 0));
    const swipe = await inputPage.evaluate(() => window.sent);
    const scroll = swipe.filter(report => report.kind === 'scroll');
    assert.equal(scroll.reduce((sum, report) => sum + report.wheel, 0), 4);
    assert.equal(scroll.filter(report => report.pan < 0).reduce((sum, report) => sum + report.pan, 0), -4,
      'A swipe left must send negative horizontal pan');
    assert.equal(scroll.filter(report => report.pan > 0).reduce((sum, report) => sum + report.pan, 0), 4,
      'A swipe right must send positive horizontal pan');
    assert.equal(scroll.filter(report => report.start).length, 3, 'Each swipe must start one scroll sequence');
    assert(scroll.every(report => report.x === 5000 && report.y === 5000), 'Swipe must scroll at the initial pointer position');
    assert(!swipe.some(report => report.kind === 'click' || report.buttons), 'Swipe must never send a press or click');

    await inputPage.mouse.move(200, 140);
    await inputPage.evaluate(() => { window.sent.length = 0; });
    await inputPage.mouse.down();
    await inputPage.waitForFunction(() => window.sent.some(report => report.buttons === 1));
    await inputPage.mouse.move(240, 140);
    await inputPage.mouse.up();
    await inputPage.waitForFunction(() => window.sent.some(report => report.x === 3500 && report.buttons === 0));
    const selection = await inputPage.evaluate(() => window.sent);
    assert(selection.some(report => report.kind === 'pointer' && report.buttons === 1 &&
      report.x === 2500 && report.y === 2500), 'Press-and-hold must begin selection before moving');
    assert(!selection.some(report => report.kind === 'scroll' || report.kind === 'click'));
    await inputPage.keyboard.press('Alt+b');
    const chord = await inputPage.evaluate(() => window.sent.filter(report => report.kind === 'key')
      .map(report => [report.usage,report.down]));
    assert.deepEqual(chord, [[0xe2,true],[5,true],[5,false],[0xe2,false]],
      'Alt must still work in forwarded keyboard shortcuts');
    console.log(`PASS iOS paced swipe input, wheel direction, text-selection input and shortcuts at ${dpi * 100}% DPI`);
    await context.close();
  }
} finally {
  await browser.close();
}
