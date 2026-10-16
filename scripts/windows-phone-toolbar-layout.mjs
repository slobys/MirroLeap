#!/usr/bin/env node
/**
 * Real Edge/Chromium geometry regression for the native phone-controls page.
 * The production phone-controls.html and CSS are loaded unchanged; only the
 * test host's Tauri IPC boundary is mocked with fixed mode booleans.
 */
import { chromium } from 'playwright-core';
import { spawn } from 'node:child_process';
import assert from 'node:assert/strict';
import { setTimeout as sleep } from 'node:timers/promises';

const origin='http://127.0.0.1:1420';
const vite=spawn(process.execPath,
  ['node_modules/vite/bin/vite.js','dev','--host','127.0.0.1','--port','1420','--strictPort'],
  {stdio:['ignore','pipe','pipe'],shell:false});
let output='';
vite.stdout.on('data',s=>{output+=s.toString().slice(-500)});
vite.stderr.on('data',s=>{output+=s.toString().slice(-500)});
let browser;

async function waitForServer() {
  for(let i=0;i<60;i++) {
    if(vite.exitCode!==null) throw Error('Vite failed: '+output);
    try {
      const res=await fetch(origin+'/phone-controls.html',{signal:AbortSignal.timeout(1200)});
      if(res.ok) return;
    } catch { /* dev server is still booting */ }
    await sleep(250);
  }
  throw Error('Phone controls page unavailable: '+output);
}

async function verify(context,{layout,width,height,zoom=100}) {
  const page=await context.newPage();
  const errors=[];
  page.on('pageerror',error=>errors.push(error.message));
  await page.addInitScript(()=>{
    let inking=false,shortcuts=false;
    window.__TAURI__={core:{invoke:async(command,{action})=>{
      if(command!=='device_frame_control') throw Error('Unexpected IPC command');
      if(action==='ink_status') return inking;
      if(action==='shortcuts_status') return shortcuts;
      if(action==='ink_toggle') {
        inking=!inking;
        if(inking) shortcuts=false;
        return inking;
      }
      if(action==='shortcuts_toggle') {
        shortcuts=!shortcuts;
        if(shortcuts) inking=false;
        return shortcuts;
      }
      if(action==='ink_reset') { inking=false;shortcuts=false;return false; }
      if(action.startsWith('shortcut_')) return true;
      return inking;
    }}};
  });
  await page.setViewportSize({width,height});
  await page.goto(origin+'/phone-controls.html',{waitUntil:'domcontentloaded'});
  await page.evaluate(([mode,zoom])=>{
    window.scrcpyControls.setLayout(mode,zoom);
  },[layout,zoom]);
  const open=layout==='side-right' || layout==='side-left' ||
    layout==='stack-right' || layout==='stack-left' ||
    layout==='short-ink-right' || layout==='short-ink-left' ||
    layout==='tight-ink-right' || layout==='tight-ink-left';
  if(open) {
    await page.locator('#shortcut-toggle').click();
    await page.waitForFunction(()=>document.body.classList.contains('shortcuts'));
  }

  const result=await page.evaluate(()=>{
    const windowW=innerWidth,windowH=innerHeight;
    const rect=e=>{
      const r=e.getBoundingClientRect();
      return {left:r.left,right:r.right,top:r.top,bottom:r.bottom,
        width:r.width,height:r.height};
    };
    const rail=document.querySelector('.rail');
    const palette=document.querySelector('#shortcut-tools');
    const buttons=[...document.querySelectorAll('.rail button,.shortcut-grid button')];
    return {
      rail:rect(rail),
      palette:rect(palette),
      paletteDisplay:getComputedStyle(palette).display,
      buttons:buttons.map(button=>({id:button.id,rect:rect(button)})),
      viewport:{width:windowW,height:windowH},
    };
  });
  const label=layout+' '+width+'x'+height+' zoom='+zoom;
  const inside=(rect,allow=1) => rect.left>=-allow&&rect.top>=-allow&&
    rect.right<=width+allow&&rect.bottom<=height+allow;
  assert(inside(result.rail),label+' clipped rail: '+JSON.stringify(result));
  assert.equal(result.buttons.filter(b=>b.id==='shortcut-toggle').length,1);
  const railButtons=result.buttons.filter(b=>['drag','minimize','close','ink-toggle','shortcut-toggle','restore'].includes(b.id));
  assert.equal(railButtons.length,6,label+' missing rail buttons');
  for(const b of railButtons) assert(inside(b.rect),label+' clipped rail button '+b.id);
  if(open) {
    assert.equal(result.paletteDisplay,'flex',label+' palette hidden');
    assert(inside(result.palette),label+' clipped palette: '+JSON.stringify(result));
    assert.equal(result.buttons.length,14,label+' missing shortcut action');
    for(const b of result.buttons) assert(inside(b.rect),label+' clipped '+b.id);
  }
  assert.deepEqual(errors,[],label+' JS failures');
  console.log('PASS',label,'rail controls',open?'8 live shortcut targets':'compact rail');
  await page.close();
}

try {
  await waitForServer();
  browser=await chromium.launch({
    channel:process.env.MIRROLEAP_LAYOUT_BROWSER==='chromium'?undefined:'msedge',
    headless:true,args:['--no-first-run','--disable-extensions'],
  });
  for(const scale of [1,1.5]) {
    const context=await browser.newContext({deviceScaleFactor:scale});
    for(const sample of [
      {layout:'compact',width:62,height:322},
      {layout:'short-right',width:168,height:118},
      {layout:'side-right',width:268,height:322},
      {layout:'side-left',width:268,height:322},
      {layout:'stack-right',width:168,height:540},
      {layout:'stack-left',width:168,height:540},
      {layout:'short-ink-right',width:374,height:212},
      {layout:'short-ink-left',width:374,height:212},
      {layout:'tight-ink-right',width:374,height:136},
      {layout:'tight-ink-left',width:374,height:136},
    ]) await verify(context,sample);
    await context.close();
  }
} finally {
  await browser?.close();
  vite.kill('SIGTERM');
}
