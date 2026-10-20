// MirroCreator native SDL click visualization. This canvas is ALWAYS
// mouse-through. Do not take pointer capture or copy phone pixels here.
(() => {
  'use strict';
  const canvas = document.getElementById('creator-effects');
  const ctx = canvas?.getContext('2d',{alpha:true});
  if (!ctx) return;
  let enabled = false;
  let active = [];
  let pending = false;
  const lifeMs = 580;
  function fit() {
    const box = canvas.getBoundingClientRect();
    const dpi = Math.min(3,Math.max(1,window.devicePixelRatio||1));
    const w = Math.max(1,Math.round(box.width*dpi));
    const h = Math.max(1,Math.round(box.height*dpi));
    if (w !== canvas.width || h !== canvas.height) {
      canvas.width=w;canvas.height=h;
      active=[];
    }
  }
  function paint(now) {
    pending=false;
    ctx.clearRect(0,0,canvas.width,canvas.height);
    if (!enabled) {active=[];return;}
    active=active.filter(item=>now-item.start<lifeMs && now>=item.start);
    for (const item of active) {
      const progress=(now-item.start)/lifeMs;
      const scale=Math.min(canvas.width,canvas.height);
      const radius=scale*(.022+progress*.075);
      ctx.beginPath();
      ctx.arc(item.x*canvas.width,item.y*canvas.height,radius,0,Math.PI*2);
      ctx.strokeStyle='rgba(45,220,182,'+(1-progress).toFixed(3)+')';
      ctx.lineWidth=Math.max(2,scale*.009*(1-progress*.65));
      ctx.stroke();
    }
    if (active.length) redraw();
  }
  function redraw() {
    if (pending) return;
    pending=true;
    requestAnimationFrame(paint);
  }
  new ResizeObserver(fit).observe(canvas);
  window.addEventListener('resize',fit);
  new MutationObserver(changes=>{
    if (changes.some(entry=>entry.attributeName==='data-orientation')) {
      active=[];fit();ctx.clearRect(0,0,canvas.width,canvas.height);
    }
  }).observe(document.documentElement,{attributes:true,attributeFilter:['data-orientation']});
  window.scrcpyCreator=Object.freeze({
    ready:true,
    isClickEnabled(){return enabled;},
    setClickEnabled(value) {
      enabled=value===true;
      if (!enabled) {active=[];ctx.clearRect(0,0,canvas.width,canvas.height);}
      return enabled;
    },
    flashClick(x,y) {
      if (!enabled || !Number.isFinite(x) || !Number.isFinite(y) ||
          x<0 || y<0 || x>1 || y>1) return false;
      fit();
      active.push({x,y,start:performance.now()});
      if (active.length>8) active.shift();
      redraw();
      return true;
    },
    reset(){enabled=false;active=[];ctx.clearRect(0,0,canvas.width,canvas.height);},
  });
  fit();
})();
