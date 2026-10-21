import {readFileSync} from 'node:fs';
import {join} from 'node:path';
import {JSDOM} from 'jsdom';
import {describe,expect,it} from 'vitest';

const frame = readFileSync(join(process.cwd(),'public/frame.html'),'utf8');
const presenter = readFileSync(join(process.cwd(),'public/present-overlay.js'),'utf8');

function boot() {
  const operations = [];
  const contexts = new Map();
  const dom = new JSDOM(frame,{
    url:'http://localhost/frame.html',runScripts:'dangerously',pretendToBeVisual:true,
    beforeParse(window) {
      window.ResizeObserver = class {observe(){} disconnect(){}};
      window.requestAnimationFrame = cb => {cb(0);return 1;};
      window.HTMLCanvasElement.prototype.getBoundingClientRect = () =>
        ({left:0,top:0,width:300,height:500,right:300,bottom:500});
      window.HTMLCanvasElement.prototype.getContext = function() {
        if (!contexts.has(this.id)) {
          const ctx = {};
          for (const method of ['save','restore','clearRect','fillRect','beginPath',
            'moveTo','lineTo','arc','ellipse','clip','fill','stroke','drawImage','strokeText','fillText']) {
            ctx[method] = (...args) => operations.push([this.id,method,...args]);
          }
          contexts.set(this.id,ctx);
        }
        return contexts.get(this.id);
      };
      window.HTMLCanvasElement.prototype.setPointerCapture = () => {};
      window.HTMLCanvasElement.prototype.hasPointerCapture = () => true;
      window.HTMLCanvasElement.prototype.releasePointerCapture = () => {};
      window.HTMLCanvasElement.prototype.toDataURL = () => 'data:image/png;base64,iVBORw0KGgo=';
      window.Image = class MockAndroidSnapshot {
        width=600;
        height=1000;
        set src(value) {
          if (!value.startsWith('data:image/png;base64,')) throw Error('unexpected image source');
          queueMicrotask(() => this.onload?.());
        }
      };
    },
  });
  dom.window.eval(presenter);
  return {dom,window:dom.window,operations};
}

describe('MirroPresent real phone-frame Canvas behavior', () => {
  it('starts passive; spotlight focus is normalized and pointer remains captured only when enabled', () => {
    const {dom,window,operations} = boot();
    const surface=window.document.getElementById('presentation');
    expect(window.scrcpyPresent.mode()).toBe('off');
    expect(window.document.body.classList.contains('presenting')).toBe(false);
    expect(surface).toBeTruthy();
    expect(window.scrcpyPresent.setMode('spotlight')).toBe('spotlight');
    expect(window.document.body.classList.contains('presenting')).toBe(true);
    surface.dispatchEvent(new window.MouseEvent('pointermove',{
      bubbles:true,clientX:150,clientY:250
    }));
    expect(window.scrcpyPresent.focusPoint()).toEqual({x:.5,y:.5});
    surface.dispatchEvent(new window.MouseEvent('pointermove',{
      bubbles:true,clientX:999,clientY:-10
    }));
    expect(window.scrcpyPresent.focusPoint()).toEqual({x:1,y:0});
    expect(operations.some(row=>row[0]==='presentation' && row[1]==='fillRect')).toBe(true);
    expect(operations.some(row=>row[0]==='presentation' && row[1]==='arc')).toBe(true);
    expect(window.scrcpyPresent.setMode('off')).toBe('off');
    expect(window.document.body.classList.contains('presenting')).toBe(false);
    expect(() => window.scrcpyPresent.setMode('videoCaptureAll')).toThrow(/Unknown presentation mode/);
    dom.window.close();
  });

  it('resizes spotlight and frozen magnifier with bounded wheel input and rounded aperture', async () => {
    const {dom,window,operations}=boot();
    const canvas=window.document.getElementById('presentation');
    expect(frame).toContain('clip-path: inset(0 round 7.46888% / 3.40587%)');
    expect(frame).toContain('clip-path: inset(0 round 3.40587% / 7.46888%)');
    expect(window.scrcpyPresent.size()).toBe(.22);
    expect(window.scrcpyPresent.zoom()).toBe(2.3);
    canvas.dispatchEvent(new window.WheelEvent('wheel',{deltaY:-100,bubbles:true,cancelable:true}));
    expect(window.scrcpyPresent.size()).toBe(.22);
    window.scrcpyPresent.setMode('spotlight');
    canvas.dispatchEvent(new window.WheelEvent('wheel',{deltaY:-100,bubbles:true,cancelable:true}));
    expect(window.scrcpyPresent.size()).toBeGreaterThan(.22);
    canvas.dispatchEvent(new window.WheelEvent('wheel',{deltaY:100,ctrlKey:true,bubbles:true,cancelable:true}));
    expect(window.scrcpyPresent.zoom()).toBe(2.3); // magnification applies to lens only
    for(let i=0;i<80;i++) canvas.dispatchEvent(new window.WheelEvent('wheel',{deltaY:-50,bubbles:true,cancelable:true}));
    expect(window.scrcpyPresent.size()).toBe(.43);
    for(let i=0;i<80;i++) canvas.dispatchEvent(new window.WheelEvent('wheel',{deltaY:50,bubbles:true,cancelable:true}));
    expect(window.scrcpyPresent.size()).toBe(.11);
    window.scrcpyPresent.setMode('magnifier');
    await window.scrcpyPresent.loadSnapshot('data:image/png;base64,YWJj','portrait');
    canvas.dispatchEvent(new window.WheelEvent('wheel',{deltaY:100,ctrlKey:true,bubbles:true,cancelable:true}));
    expect(window.scrcpyPresent.zoom()).toBeLessThan(2.3);
    canvas.dispatchEvent(new window.WheelEvent('wheel',{deltaY:-100,bubbles:true,cancelable:true}));
    expect(window.scrcpyPresent.size()).toBeGreaterThan(.11);
    expect(operations.some(row=>row[0]==='presentation'&&row[1]==='drawImage')).toBe(true);
    window.scrcpyPresent.resetSize();
    expect(window.scrcpyPresent.size()).toBe(.22);
    expect(window.scrcpyPresent.zoom()).toBe(2.3);
    dom.window.close();
  });

  it('labels magnifier as a still snapshot, never live video, and accepts explicit PNG only', async () => {
    const {dom,window,operations} = boot();
    window.scrcpyPresent.setMode('magnifier');
    expect(operations.some(row=>row[0]==='presentation' &&
      row[1]==='fillText' && String(row[2]).includes('请刷新'))).toBe(true);
    expect(await window.scrcpyPresent.loadSnapshot('data:image/png;base64,YWJj','portrait')).toBe(true);
    expect(operations.some(row=>row[0]==='presentation' && row[1]==='drawImage')).toBe(true);
    expect(operations.some(row=>row[0]==='presentation' &&
      row[1]==='fillText' && String(row[2]).includes('非实时'))).toBe(true);
    await expect(window.scrcpyPresent.loadSnapshot('https://evil.example/image.png','portrait')).rejects.toThrow(/Invalid/);
    await expect(window.scrcpyPresent.loadSnapshot('data:image/png;base64,YWJj','landscape')).rejects.toThrow(/orientation/);
    window.scrcpyPresent.setMode('off');
    await expect(window.scrcpyPresent.loadSnapshot('data:image/png;base64,YWJj','portrait')).rejects.toThrow(/Enable/);
    dom.window.close();
  });

  it('shades only outside the snapshot circle, cycles dark/black and clears on exit', async()=>{
    const {dom,window,operations}=boot();
    const lens=window.scrcpyPresent;
    expect(lens.privacy()).toBe('off');
    expect(()=>lens.setPrivacy('black')).toThrow(/active magnifier/);
    lens.setMode('magnifier');
    await lens.loadSnapshot('data:image/png;base64,YWJj','portrait');
    operations.length=0;
    expect(lens.setPrivacy('dim')).toBe('dim');
    expect(operations.some(row=>row[0]==='presentation' && row[1]==='fillRect' &&
      row[2]===0 && row[3]===0 && row[4]===300 && row[5]===500)).toBe(true);
    expect(operations.some(row=>row[0]==='presentation' && row[1]==='drawImage')).toBe(true);
    expect(lens.setPrivacy('black')).toBe('black');
    expect(()=>lens.setPrivacy('fake')).toThrow(/Unknown privacy/);
    lens.setMode('off');
    expect(lens.privacy()).toBe('off');
    expect(window.document.body.classList.contains('presenting')).toBe(false);
    dom.window.close();
  });

  it('live lens accepts only native relative circle geometry, hides its mask on exit',()=>{
    const {dom,window,operations}=boot();
    const lens=window.scrcpyPresent;
    lens.setMode('live');
    expect(window.document.body.classList.contains('presenting')).toBe(false);
    expect(lens.setPrivacy('black')).toBe('black');
    expect(lens.updatePrivacyCircle({cx:2,cy:.5,rx:.2,ry:.2})).toBe(false);
    expect(lens.updatePrivacyCircle({cx:.5,cy:.5,rx:Infinity,ry:.2})).toBe(false);
    operations.length=0;
    expect(lens.updatePrivacyCircle({cx:.5,cy:.5,rx:.25,ry:.12})).toBe(true);
    expect(operations.some(row=>row[0]==='presentation' && row[1]==='ellipse' &&
      row[2]===150 && row[3]===250 && row[4]===75 && row[5]===60)).toBe(true);
    expect(operations.some(row=>row[0]==='presentation' && row[1]==='fillRect')).toBe(true);
    operations.length=0;
    expect(lens.updatePrivacyCircle(null)).toBe(true);
    expect(operations.some(row=>row[0]==='presentation' && row[1]==='ellipse')).toBe(false);
    lens.setMode('off');
    expect(lens.privacy()).toBe('off');
    dom.window.close();
  });

  it('clears snapshots on rotation and does not mutate existing pen marks', async () => {
    const {dom,window} = boot();
    const ink=window.document.getElementById('annotation');
    window.scrcpyInk.setEnabled(true);
    ink.dispatchEvent(new window.MouseEvent('pointerdown',{bubbles:true,button:0}));
    ink.dispatchEvent(new window.MouseEvent('pointerup',{bubbles:true,button:0}));
    expect(window.scrcpyInk.captureSnapshot().strokes).toBe(1);
    window.scrcpyInk.setEnabled(false);
    window.scrcpyPresent.setMode('magnifier');
    await window.scrcpyPresent.loadSnapshot('data:image/png;base64,YWJj','portrait');
    expect(window.scrcpyInk.captureSnapshot().strokes).toBe(1);
    window.scrcpyFrame.setOrientation('landscape');
    await new Promise(resolve=>setTimeout(resolve,0));
    expect(window.scrcpyPresent.mode()).toBe('magnifier');
    // Orientation change deliberately invalidates ALL old pen marks, as the
    // existing frame.html design already requires for coordinate correctness.
    expect(() => window.scrcpyInk.captureSnapshot()).toThrow(/no pen strokes/i);
    dom.window.close();
  });
});
