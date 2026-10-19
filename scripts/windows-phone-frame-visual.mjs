#!/usr/bin/env node
/**
 * Physical-pixel visual gate for the real SVG phone body in Windows Edge.
 *
 * GDI Win32 HRGN intentionally sits OUTSIDE the visible SVG outline; only
 * Chromium/WebView2 should rasterize the final alpha edge. Previous builds
 * accidentally clipped the SVG's fractional AA and stacked seven dark rings.
 * This checks all four corners at 100/125/150% scaling, both orientations.
 *
 * The native SDL/DWM window is not emulated here; Win32 build, HRGN geometry
 * tests and Android real-device acceptance remain separate requirements.
 */
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { resolve } from 'node:path';
import { chromium } from 'playwright-core';

const cases = [
  {name:'portrait', svg:'iphone-black-portrait.svg', width:440, height:946, artW:1000, artH:2150},
  {name:'landscape', svg:'iphone-black-landscape.svg', width:946, height:440, artW:2150, artH:1000},
];
const browser = await chromium.launch({
  channel:process.env.MIRROLEAP_LAYOUT_BROWSER === 'chromium' ? undefined : 'msedge',
  headless:true,
  args:['--no-first-run','--disable-extensions'],
});

try {
  for (const dpi of [1,1.25,1.5]) {
    const context = await browser.newContext({deviceScaleFactor:dpi});
    const page = await context.newPage();
    for (const test of cases) {
      const svg = readFileSync(resolve('public/device-frame',test.svg));
      const src = 'data:image/svg+xml;base64,' + svg.toString('base64');
      const width = Math.round(test.width * dpi);
      const height = Math.round(test.height * dpi);
      const pixelStats = await page.evaluate(async ({src,width,height,artW,artH}) => {
        const image = new Image();
        image.src = src;
        await image.decode();
        const canvas = document.createElement('canvas');
        canvas.width = width;
        canvas.height = height;
        const ctx = canvas.getContext('2d', {alpha:true,willReadFrequently:true});
        ctx.clearRect(0,0,width,height);
        ctx.drawImage(image,0,0,width,height);
        const bytes = ctx.getImageData(0,0,width,height).data;
        const alpha = (x,y) => bytes[4*(y*width+x)+3];
        const arcX = (1+76*(1-1/Math.SQRT2))*width/artW;
        const arcY = (1+76*(1-1/Math.SQRT2))*height/artH;
        const positions = [
          [arcX,arcY],
          [width-1-arcX,arcY],
          [arcX,height-1-arcY],
          [width-1-arcX,height-1-arcY],
        ];
        const samples = [];
        for (const [cx,cy] of positions) {
          let partial=0;
          for (let dy=-3;dy<=3;dy++) {
            for(let dx=-3;dx<=3;dx++) {
              const x=Math.round(cx)+dx, y=Math.round(cy)+dy;
              if(x<0||y<0||x>=width||y>=height) continue;
              const a=alpha(x,y);
              if(a>8&&a<248) partial++;
            }
          }
          samples.push(partial);
        }
        const center = alpha(Math.round(width/2),Math.round(height/2));
        const rim = alpha(Math.round(width * 5/artW),Math.round(height/2));
        const topLeft = alpha(0,0);
        return {samples,center,rim,topLeft};
      }, {src,width,height,artW:test.artW,artH:test.artH});
      assert.equal(pixelStats.center,0,
        `${test.name} ${dpi}x: alpha screen hole was painted over`);
      assert.ok(pixelStats.rim>200,
        `${test.name} ${dpi}x: missing continuous bezel material (${pixelStats.rim})`);
      assert.equal(pixelStats.topLeft,0,
        `${test.name} ${dpi}x: a square background leaks from the outer phone corner`);
      assert.ok(pixelStats.samples.every(value=>value>=1),
        `${test.name} ${dpi}x: no fractional-alpha smoothing at one of four outer arcs: ${pixelStats.samples}`);
      console.log(`Edge alpha verified: ${test.name}, ${dpi*100}% DPI, ${width}x${height}, edge AA pixels=${pixelStats.samples.join('/')}`);
    }
    await page.close();
    await context.close();
  }
} finally {
  await browser.close();
}
