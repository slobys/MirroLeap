import {readFileSync} from 'node:fs';
import {join} from 'node:path';
import {JSDOM} from 'jsdom';
import {describe,it,expect} from 'vitest';

const frame=readFileSync(join(process.cwd(),'public/frame.html'),'utf8');
const ripple=readFileSync(join(process.cwd(),'public/creator-effects.js'),'utf8');
const presenter=readFileSync(join(process.cwd(),'public/present-overlay.js'),'utf8');

function createFrame() {
  const draw=[];
  let raf=null;
  const ctx=new Proxy({},{
    get(target,key) {
      return target[key] || ((...args) => {draw.push({method:String(key),args});});
    },
    set(target,key,value) {target[key]=value;return true;}
  });
  const dom=new JSDOM(frame,{
    url:'http://localhost/frame.html',runScripts:'dangerously',
    pretendToBeVisual:true,
    beforeParse(window) {
      window.ResizeObserver=class {observe(){} disconnect(){}};
      window.requestAnimationFrame=callback=>{raf=callback;return 1;};
      window.HTMLCanvasElement.prototype.getContext=()=>ctx;
      window.HTMLCanvasElement.prototype.toDataURL=()=> 'data:image/png;base64,eA==';
      window.HTMLCanvasElement.prototype.setPointerCapture=()=>{};
      window.HTMLCanvasElement.prototype.hasPointerCapture=()=>false;
      window.HTMLCanvasElement.prototype.releasePointerCapture=()=>{};
      window.Element.prototype.getBoundingClientRect=()=>({
        x:0,y:0,left:0,top:0,width:200,height:400,right:200,bottom:400
      });
      window.Image=class{
        width=200;height=400;
        set src(value) {if(value)queueMicrotask(()=>this.onload?.());}
      };
    }
  });
  dom.window.eval(ripple);
  dom.window.eval(presenter);
  const annotation=dom.window.document.getElementById('annotation');
  const place=(type,x,y) => {
    annotation.dispatchEvent(new dom.window.MouseEvent(type,{
      bubbles:true,cancelable:true,button:0,clientX:x,clientY:y
    }));
  };
  return {dom,draw,annotation,place,getFrame:()=>raf};
}

describe('actual MirroCreator transparent phone overlay',()=>{
  it('draws normalized arrow, rectangle, numbered steps, and opaque mask on the ink canvas',()=>{
    const {dom,draw,place}=createFrame();
    const ink=dom.window.scrcpyInk;
    expect(ink.ready).toBe(true);
    ink.setEnabled(true);
    ink.setTool('rect');
    place('pointerdown',20,30);place('pointermove',95,190);place('pointerup',95,190);
    expect(draw.some(call=>call.method==='strokeRect')).toBe(true);
    ink.setTool('arrow');
    place('pointerdown',40,80);place('pointermove',140,260);place('pointerup',140,260);
    expect(draw.some(call=>call.method==='lineTo')).toBe(true);
    ink.setTool('step');
    place('pointerdown',60,100);place('pointerup',60,100);
    place('pointerdown',70,120);place('pointerup',70,120);
    expect(draw.some(call=>call.method==='fillText'&&call.args[0]==='1')).toBe(true);
    expect(draw.some(call=>call.method==='fillText'&&call.args[0]==='2')).toBe(true);
    ink.setTool('mask');
    place('pointerdown',20,30);place('pointermove',110,180);place('pointerup',110,180);
    expect(draw.some(call=>call.method==='fillRect'&&call.args[2]>0&&call.args[3]>0))
      .toBe(true);
    expect(ink.captureSnapshot().strokes).toBe(5);
    ink.setEnabled(false);
    expect(dom.window.document.body.classList.contains('inking')).toBe(false);
    expect(ink.captureSnapshot().strokes).toBe(5);
    ink.undo();
    expect(ink.captureSnapshot().strokes).toBe(4);
    ink.clear();
    expect(()=>ink.captureSnapshot()).toThrow('There are no pen strokes');
    dom.window.close();
  });

  it('clears old mark positions on orientation changes and refuses unknown tools',()=>{
    const {dom,place}=createFrame();
    const ink=dom.window.scrcpyInk;
    ink.setEnabled(true);
    ink.setTool('step');
    place('pointerdown',100,200);place('pointerup',100,200);
    expect(ink.captureSnapshot().strokes).toBe(1);
    ink.setTool('script-injection');
    expect(ink.currentTool()).toBe('step');
    dom.window.scrcpyFrame.setOrientation('landscape');
    expect(()=>ink.captureSnapshot()).toThrow('There are no pen strokes');
    dom.window.close();
  });

  it('keeps mouse click ripples pass-through and default OFF',()=>{
    const {dom}=createFrame();
    const click=dom.window.scrcpyCreator;
    expect(click.isClickEnabled()).toBe(false);
    expect(click.flashClick(.3,.4)).toBe(false);
    expect(click.setClickEnabled(true)).toBe(true);
    expect(click.flashClick(.3,.4)).toBe(true);
    expect(click.flashClick(-1,.4)).toBe(false);
    expect(click.flashClick(NaN,.4)).toBe(false);
    expect(dom.window.getComputedStyle(dom.window.document.getElementById('creator-effects'))
      .pointerEvents).toBe('none');
    click.reset();
    expect(click.isClickEnabled()).toBe(false);
    dom.window.close();
  });

  it('uses genuine fresh-frame updates and native mouse-through focus in live mode',async()=>{
    const {dom,draw,getFrame}=createFrame();
    const view=dom.window.scrcpyPresent;
    expect(view.mode()).toBe('off');
    expect(view.setMode('live')).toBe('live');
    expect(view.setFocus(.98,.6)).toBe(true);
    expect(view.setFocus(-.5,.6)).toBe(false);
    const png='data:image/png;base64,eA==';
    expect(await view.loadSnapshot(png,'portrait')).toBe(true);
    expect(view.liveStats().frames).toBe(1);
    expect(await view.loadSnapshot(png,'portrait')).toBe(true);
    expect(view.liveStats().frames).toBe(2);
    // Counter and measured update timestamp are visible even when the home
    // screen pixels themselves are unchanged.
    getFrame()(dom.window.performance.now());
    expect(draw.some(call=>call.method==='fillText'&&
      String(call.args[0]).includes('2帧已更新'))).toBe(true);
    expect(view.setMode('off')).toBe('off');
    expect(view.liveStats().frames).toBe(0);
    expect(view.setFocus(.5,.5)).toBe(false);
    await expect(view.loadSnapshot(png,'portrait')).rejects.toThrow('Enable a magnifier');
    dom.window.close();
  });

  it('keeps both sides of a zoom lens inside the phone while focusing edge icons',async()=>{
    const {dom,draw,getFrame}=createFrame();
    const view=dom.window.scrcpyPresent;
    const canvas=dom.window.document.getElementById('presentation');
    view.setMode('magnifier');
    expect(await view.loadSnapshot('data:image/png;base64,eA==','portrait')).toBe(true);
    for (const [px,py] of [[1,1],[1,200],[199,100],[199,399],[100,399]]) {
      draw.length=0;
      canvas.dispatchEvent(new dom.window.MouseEvent('pointermove',{
        bubbles:true,clientX:px,clientY:py
      }));
      getFrame()(dom.window.performance.now());
      const calls=draw.filter(call=>call.method==='drawImage');
      expect(calls).toHaveLength(1);
      const circle=draw.filter(call=>call.method==='arc').at(-1).args;
      const [cx,cy,radius]=circle;
      expect(cx-radius).toBeGreaterThanOrEqual(0);
      expect(cy-radius).toBeGreaterThanOrEqual(0);
      expect(cx+radius).toBeLessThanOrEqual(canvas.width);
      expect(cy+radius).toBeLessThanOrEqual(canvas.height);
      const [source,sx,sy,sw,sh,dx,dy,dw,dh]=calls[0].args;
      expect(source.width).toBe(200);
      expect(sx).toBeGreaterThanOrEqual(0);
      expect(sy).toBeGreaterThanOrEqual(0);
      expect(sx+sw).toBeLessThanOrEqual(source.width);
      expect(sy+sh).toBeLessThanOrEqual(source.height);
      expect(dx).toBeGreaterThanOrEqual(cx-radius-0.01);
      expect(dy).toBeGreaterThanOrEqual(cy-radius-0.01);
      expect(dx+dw).toBeLessThanOrEqual(cx+radius+0.01);
      expect(dy+dh).toBeLessThanOrEqual(cy+radius+0.01);
    }
    dom.window.close();
  });
});
