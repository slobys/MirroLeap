import { readFileSync } from 'node:fs';
import { join } from 'node:path';
import { JSDOM } from 'jsdom';
import { describe, expect, it } from 'vitest';

const html = readFileSync(join(process.cwd(), 'public/frame.html'), 'utf8');
function openInkOverlay() {
  const drawing = [];
  const dom = new JSDOM(html, {
    url: 'http://localhost/frame.html',
    runScripts: 'dangerously',
    pretendToBeVisual: true,
    beforeParse(window) {
      window.ResizeObserver = class { observe() {} disconnect() {} };
      window.HTMLCanvasElement.prototype.getContext = () => ({
        save() {}, restore() {}, beginPath() {}, moveTo() {}, lineTo() {},
        arc() {}, fill() {}, stroke() {}, clearRect() {},
      });
      window.HTMLCanvasElement.prototype.toDataURL = (format) => {
        expect(format).toBe('image/png');
        drawing.push('captured-canvas-bitmap');
        return 'data:image/png;base64,iVBORw0KGgo=';
      };
      window.HTMLCanvasElement.prototype.setPointerCapture = () => {};
      window.HTMLCanvasElement.prototype.hasPointerCapture = () => true;
      window.HTMLCanvasElement.prototype.releasePointerCapture = () => {};
    },
  });
  return {dom, window:dom.window, drawing};
}

describe('same WebView2 pen canvas export', () => {
  it('does not export an empty pen overlay as a misleading annotated screenshot', () => {
    const {dom, window} = openInkOverlay();
    expect(() => window.scrcpyInk.captureSnapshot()).toThrow(/no pen strokes/i);
    dom.window.close();
  });

  it('captures the actual bitmap after drawing and leaves strokes intact', () => {
    const {dom, window, drawing} = openInkOverlay();
    const canvas = window.document.getElementById('annotation');
    window.scrcpyInk.setEnabled(true);
    canvas.dispatchEvent(new window.MouseEvent('pointerdown', {bubbles:true,button:0,clientX:4,clientY:8}));
    canvas.dispatchEvent(new window.MouseEvent('pointerup', {bubbles:true,button:0,clientX:4,clientY:8}));
    const first = window.scrcpyInk.captureSnapshot();
    expect(first.png).toMatch(/^data:image\/png;base64,/);
    expect(first.orientation).toBe('portrait');
    expect(first.strokes).toBe(1);
    expect(drawing).toHaveLength(1);
    // Snapshot does not clear or leave drawing mode.
    expect(window.scrcpyInk.isEnabled()).toBe(true);
    expect(window.scrcpyInk.captureSnapshot().strokes).toBe(1);
    window.scrcpyInk.undo();
    expect(() => window.scrcpyInk.captureSnapshot()).toThrow(/no pen strokes/i);
    dom.window.close();
  });

  it('rejects old portrait annotations after a phone orientation change', () => {
    const {dom, window} = openInkOverlay();
    const canvas = window.document.getElementById('annotation');
    window.scrcpyInk.setEnabled(true);
    canvas.dispatchEvent(new window.MouseEvent('pointerdown', {bubbles:true,button:0}));
    canvas.dispatchEvent(new window.MouseEvent('pointerup', {bubbles:true,button:0}));
    expect(window.scrcpyInk.captureSnapshot().strokes).toBe(1);
    expect(window.scrcpyFrame.setOrientation('landscape')).toBe('landscape');
    expect(() => window.scrcpyInk.captureSnapshot()).toThrow(/no pen strokes/i);
    dom.window.close();
  });
});
