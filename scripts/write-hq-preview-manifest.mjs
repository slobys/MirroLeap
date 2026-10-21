#!/usr/bin/env node
/**
 * HQ Preview ONLY: emit a version-tag-scoped Tauri updater manifest from
 * an actual pinned-key minisign-signed NSIS installer. This never publishes
 * or points to the official MirroLeap stable update endpoint.
 *
 * Usage:
 * node scripts/write-hq-preview-manifest.mjs VERSION TAG EXE EXE.sig OUT
 */
import assert from 'node:assert/strict';
import {readFileSync, writeFileSync} from 'node:fs';
import {basename} from 'node:path';
import {verifyUpdaterArtifact} from './verify-updater-signature.mjs';

const [version, tag, exe, signatureFile, output] = process.argv.slice(2);
assert.match(version || '',/^\d+\.\d+\.\d+$/,'Expected a strict numeric preview version');
assert.equal(typeof tag,'string','Missing preview tag');
assert.match(tag,/^hq-preview-v\d+\.\d+\.\d+-run[1-9]\d*$/,'Not a canonical HQ Preview tag');
assert.ok(tag.startsWith(`hq-preview-v${version}-run`),'Tag and package version differ');
const name=`MirroLeap.HQ.Preview_${version}_x64-setup.exe`;
assert.equal(basename(exe||''),name,'Must sign exactly the published-name EXE');
assert.equal(basename(signatureFile||''),name+'.sig');
assert.ok(output,'Manifest path is required');
// The stable key is pinned in the desktop app and inherited by the HQ
// preview's Tauri config. Never read or display the private signing key.
verifyUpdaterArtifact(exe,signatureFile,'src-tauri/tauri.conf.json');
const signature=readFileSync(signatureFile,'utf8').trim();
assert.ok(signature.length>=80 && signature.length<=2000,'Bad minisign signature length');

const release=`https://github.com/slobys/MirroLeap/releases/download/${tag}/${name}`;
const manifest={
  version,
  notes:'MirroLeap HQ Preview signed Windows NSIS installer. This does NOT update the stable app.',
  pub_date:new Date().toISOString(),
  platforms:{'windows-x86_64':{url:release,signature}},
};
writeFileSync(output,JSON.stringify(manifest,null,2)+'\n');
console.log('PASS signed HQ Preview manifest, separate release and pinned Tauri key:',tag);
