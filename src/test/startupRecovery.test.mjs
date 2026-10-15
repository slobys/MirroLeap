import { readFileSync } from 'node:fs';
import { join } from 'node:path';
import { JSDOM } from 'jsdom';
import { describe, expect, it } from 'vitest';

const root = process.cwd();
const html = readFileSync(join(root, 'index.html'), 'utf8');

describe('MirroLeap visible failure fallback', () => {
  it('shows an explanation even when JavaScript never runs', () => {
    const dom = new JSDOM(html, { runScripts: 'outside-only' });
    expect(dom.window.document.getElementById('root')?.childElementCount).toBe(0);
    const fallback = dom.window.document.getElementById('startup-fallback');
    expect(fallback).toBeTruthy();
    expect(fallback.textContent).toContain('MirroLeap');
    expect(fallback.textContent).toContain('未能正常加载');
    dom.window.close();
  });

  it('requires a real React DOM commit and an executable WebView2 smoke gate', () => {
    const entry = readFileSync(join(root, 'src/main.tsx'), 'utf8');
    const workflow = readFileSync(join(root, '.github/workflows/desktop-ci.yml'), 'utf8');
    const test = readFileSync(join(root, 'scripts/windows-smoke-cdp.mjs'), 'utf8');
    const accessibility = readFileSync(join(root, 'scripts/windows-smoke-uia.ps1'), 'utf8');
    expect(entry).toContain('MutationObserver');
    expect(entry).toContain('rootElement.firstElementChild');
    expect(workflow).toContain('./scripts/windows-smoke-webview.ps1');
    expect(test).toContain('state.homeHeader && state.phoneList && state.mirrorControls');
    expect(accessibility).toContain('System.Windows.Automation.AutomationElement');
    expect(accessibility).toContain('Screen mirroring');
    expect(accessibility).toContain('My devices');
  });
});
