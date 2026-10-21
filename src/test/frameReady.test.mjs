import {readFileSync} from 'node:fs';
import {join} from 'node:path';
import {JSDOM} from 'jsdom';
import {describe,expect,it,vi} from 'vitest';

const markup=readFileSync(join(process.cwd(),'public/frame.html'),'utf8');
const readyScript=readFileSync(join(process.cwd(),'public/frame-ready.js'),'utf8');
const tick=(ms=0)=>new Promise(resolve=>setTimeout(resolve,ms));
function boot({stalledRaf=false,badBackground=false,allowDecode=true}={}) {
  const calls=[];
  let finishDecode;
  const decode= new Promise((resolve,reject)=> {
    finishDecode=()=>allowDecode?resolve():reject(new Error('SVG decode failed'));
  });
  const dom=new JSDOM(markup,{
    url:'http://localhost/frame.html',runScripts:'dangerously',pretendToBeVisual:true,
    beforeParse(window) {
      window.ResizeObserver=class{observe(){} disconnect(){}};
      window.HTMLCanvasElement.prototype.getContext=()=>({
        clearRect(){},setTransform(){},fillRect(){},strokeRect(){},beginPath(){},
        save(){},restore(){},arc(){},fill(){},stroke(){},drawImage(){},fillText(){}
      });
      Object.defineProperty(window.HTMLImageElement.prototype,'naturalWidth',{get(){return 1000;}});
      Object.defineProperty(window.HTMLImageElement.prototype,'complete',{get(){return true;}});
      window.HTMLImageElement.prototype.decode=()=>decode;
      window.__TAURI__={core:{invoke:async command=>{calls.push(command);}}};
      if (stalledRaf) window.requestAnimationFrame=()=>1;
      else window.requestAnimationFrame=cb=>{window.setTimeout(()=>cb(0),0);return 1;};
    }
  });
  const w=dom.window;
  w.scrcpyInk={ready:true};
  w.scrcpyPresent={ready:true};
  if (badBackground) w.document.body.style.setProperty('background','#fff','important');
  w.eval(readyScript);
  return {dom,calls,finishDecode};
}

describe('hidden transparent phone frame startup handshake',()=>{
  it('does not report ready until SVG decode and initial paint have completed',async()=>{
    const {dom,calls,finishDecode}=boot();
    await tick(30);
    expect(calls).toEqual([]);
    finishDecode();
    await vi.waitFor(()=>expect(calls).toEqual(['device_frame_page_ready']),{timeout:800});
    dom.window.close();
  });
  it('uses a bounded fallback when rAF is throttled for a hidden window',async()=>{
    const {dom,calls,finishDecode}=boot({stalledRaf:true});
    finishDecode();
    await tick(50);
    expect(calls).toEqual([]);
    await vi.waitFor(()=>expect(calls).toEqual(['device_frame_page_ready']),{timeout:800});
    dom.window.close();
  });
  it('never exposes an opaque or failed-to-decode frame as ready',async()=>{
    const opaque=boot({badBackground:true});
    opaque.finishDecode();
    await tick(40);
    expect(opaque.calls).toEqual([]);
    opaque.dom.window.close();
    const failed=boot({allowDecode:false});
    failed.finishDecode();
    await tick(40);
    expect(failed.calls).toEqual([]);
    failed.dom.window.close();
  });
});
