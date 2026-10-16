#!/usr/bin/env node
// Generate the official stable Tauri updater endpoint only from this job's
// final signed-and-hashed NSIS artifact. Never take package URLs from input.
import { readFileSync, writeFileSync } from 'node:fs';
import assert from 'node:assert/strict';
import { basename } from 'node:path';
import { verifyUpdaterArtifact } from './verify-updater-signature.mjs';
const [version, installerPath, sigPath, outputPath] = process.argv.slice(2);
assert.match(version ?? '', /^\d+\.\d+\.\d+$/);
assert(installerPath && sigPath && outputPath,
  'Expected version, final NSIS installer, signed NSIS signature, output');
assert.equal(basename(installerPath), `MirroLeap_${version}_x64-setup.exe`);
assert.equal(basename(sigPath), `MirroLeap_${version}_x64-setup.exe.sig`);
const signature = readFileSync(sigPath, 'utf8').trim();
assert(signature.length >= 80 && signature.length < 2000, 'Invalid minisign signature');
// Release metadata must never point at an installer whose bytes differ from
// what the pinned Tauri public key signed.
verifyUpdaterArtifact(installerPath, sigPath);
const repo = 'slobys/MirroLeap';
const tag = `v${version}`;
const file = `MirroLeap_${version}_x64-setup.exe`;
const manifest = {
  version,
  notes: 'MirroLeap signed Windows installer update. Verify and install from the application.',
  pub_date: new Date().toISOString(),
  platforms: {
    'windows-x86_64': {
      url: `https://github.com/${repo}/releases/download/${tag}/${file}`,
      signature,
    },
  },
};
writeFileSync(outputPath, JSON.stringify(manifest, null, 2) + '\n');
console.log('Prepared signed, canonical Tauri updater manifest:', version);
