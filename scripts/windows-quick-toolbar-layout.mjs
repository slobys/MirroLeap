#!/usr/bin/env node
// Real Chromium/Edge layout verification for the new quick-action flyout.
// Exercises actual phone-controls.html without scrcpy or Android hardware.
import assert from 'node:assert/strict';
import {spawn} from 'node:child_process';
import {setTimeout as delay} from 'node:timers/promises';
import {chromium} from 'playwright-core';

const origin = 'http://127.0.0.1:1420';
const vite = spawn(process.execPath,
  ['node_modules/vite/bin/vite.js','dev','--host','127.0.0.1','--port','1420','--strictPort'],
  {stdio:['ignore','pipe','pipe'],shell:false});
let browser;
async function waitForVite() {
  for (let i=0;i<60;i++) {
    if (vite.exitCode !== null) throw Error('Vite exited');
    try {
      const response=await fetch(origin+'/phone-controls.html',{signal:AbortSignal.timeout(1300)});
      if(response.ok)return;
    }catch { /* startup race */ }
    await delay(200);
  }
  throw Error('Toolbar fixture not available');
}
const configs = [
  ['compact',62,318,false],
  ['side-right',268,318,true],
  ['side-left',268,318,true],
  ['stack-right',168,534,true],
  ['stack-left',168,534,true],
  ['short-right',168,118,false],
  ['short-left',168,118,false],
  ['short-ink-right',374,212,true],
  ['short-ink-left',374,212,true],
  ['tight-ink-right',374,136,true],
  ['tight-ink-left',374,136,true],
];
const rect = el => {
  const r=el.getBoundingClientRect();
  return {x:r.x,y:r.y,right:r.right,bottom:r.bottom,width:r.width,height:r.height};
};
try {
  await waitForVite();
  browser = await chromium.launch({
    channel:process.env.MIRROLEAP_LAYOUT_BROWSER==='chromium'?undefined:'msedge',
    headless:true,args:['--no-first-run','--disable-extensions']
  });
  for(const scale of [1,1.5]){
    const context=await browser.newContext({deviceScaleFactor:scale});
    const page=await context.newPage();
    await page.addInitScript(() => {
      let quick=false;
      window.__TAURI__={core:{invoke:async (command,{action}={})=>{
        if(command!=='device_frame_control')throw Error('Unexpected command');
        if(action==='quick_toggle')quick=!quick;
        if(action==='quick_status')return quick;
        if(action.startsWith('quick_'))return quick;
        return false;
      }}};
    });
    for(const [layout,width,height,open] of configs){
      await page.setViewportSize({width,height});
      await page.goto(origin+'/phone-controls.html',{waitUntil:'domcontentloaded'});
      await page.evaluate(({layout,open})=>{
        document.body.dataset.layout=layout;
        document.body.classList.toggle('quick-open',open);
      },{layout,open});
      const result=await page.evaluate(()=>{
        const toRect=el=>{
          const r=el.getBoundingClientRect();
          return {x:r.x,y:r.y,right:r.right,bottom:r.bottom,width:r.width,height:r.height};
        };
        const palette=document.getElementById('quick-tools');
        const rail=document.querySelector('.rail');
        return {
          rail:toRect(rail),palette:toRect(palette),
          paletteDisplay:getComputedStyle(palette).display,
          buttons:[...document.querySelectorAll('.quick-grid button')].map(toRect),
          session:[...document.querySelectorAll('.session-actions button')].map(toRect),
          toggles:[...document.querySelectorAll('.annotation-actions button')].map(toRect),
          overflowX:document.documentElement.scrollWidth-document.documentElement.clientWidth,
          overflowY:document.documentElement.scrollHeight-document.documentElement.clientHeight
        };
      });
      const label=`${layout} ${width}x${height} @${scale*100}%`;
      const inside=r=>r.x>=-2 && r.y>=-2 && r.right<=width+2 && r.bottom<=height+2;
      assert(inside(result.rail),label+': rail clipped '+JSON.stringify(result.rail));
      assert(result.session.every(inside),label+': window controls clipped');
      assert(result.toggles.every(inside),label+': toolbar actions clipped');
      assert.equal(result.buttons.length,8,label+': missing shortcut controls');
      if(open){
        assert.equal(result.paletteDisplay,'flex',label+': quick palette hidden');
        assert(inside(result.palette),label+': quick palette clipped '+JSON.stringify(result.palette));
        assert(result.buttons.every(inside),label+': shortcut buttons clipped');
      } else {
        assert.equal(result.paletteDisplay,'none',label+': quick palette displayed unexpectedly');
      }
      assert(result.overflowX<=1&&result.overflowY<=1,label+': document overflow');
      console.log('PASS',label);
    }
    await context.close();
  }
}finally{
  await browser?.close();
  vite.kill('SIGTERM');
}
