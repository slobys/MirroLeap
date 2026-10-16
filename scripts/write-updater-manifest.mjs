#!/usr/bin/env node
// Generate the official stable Tauri updater endpoint only from this job's
// final signed-and-hashed NSIS artifact. Never take package URLs from input.
import { readFileSync, writeFileSync } from 'node:fs';
import assert from 'node:assert/strict';
const [version, sigPath, outputPath] = process.argv.slice(2);
assert.match(version ?? '', /^\d+\.\d+\.\d+$/);
assert(sigPath && outputPath, 'Expected version, signed NSIS signature, output');
const signature = readFileSync(sigPath, 'utf8').trim();
assert(signature.length >= 80 && signature.length < 2000, 'Invalid minisign signature');
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
