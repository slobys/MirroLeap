#!/usr/bin/env node
// Independent verification of Tauri's minisign update signatures.
// Uses only Node's built-in Ed25519 and BLAKE2b-512 implementation. The
// trusted public key is pinned in tauri.conf.json; no private key is read.
import assert from 'node:assert/strict';
import { createHash, createPublicKey, verify as verifyEd25519 } from 'node:crypto';
import { readFileSync } from 'node:fs';
import { basename, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

function decodeB64(value, label) {
  assert.match(value, /^[A-Za-z0-9+/]+={0,2}$/, `Malformed ${label} base64`);
  const bytes = Buffer.from(value, 'base64');
  assert.equal(bytes.toString('base64'), value, `Noncanonical ${label} base64`);
  return bytes;
}

export function verifyUpdaterSignature(installerBytes, encodedSignature, encodedPublicKey) {
  const publicText = decodeB64(encodedPublicKey.trim(), 'updater public key').toString('utf8');
  const publicLines = publicText.trimEnd().split(/\r?\n/);
  assert.equal(publicLines.length, 2, 'Invalid pinned minisign public key file');
  assert.ok(publicLines[0].startsWith('untrusted comment:'), 'Public key header missing');
  const keyBytes = decodeB64(publicLines[1], 'minisign public key');
  assert.equal(keyBytes.length, 42, 'Invalid minisign public key length');
  assert.ok(['Ed', 'ED'].includes(keyBytes.subarray(0, 2).toString('ascii')));

  const sigText = decodeB64(encodedSignature.trim(), 'updater signature').toString('utf8');
  const lines = sigText.trimEnd().split(/\r?\n/);
  assert.equal(lines.length, 4, 'Minisign signature must have four lines');
  assert.ok(lines[0].startsWith('untrusted comment:'), 'Signature header missing');
  assert.ok(lines[2].startsWith('trusted comment: '), 'Signature trusted comment missing');
  const signatureBytes = decodeB64(lines[1], 'minisign signature record');
  const commentSignature = decodeB64(lines[3], 'minisign comment signature');
  assert.equal(signatureBytes.length, 74, 'Invalid minisign signature record size');
  assert.equal(commentSignature.length, 64, 'Invalid minisign global signature size');
  assert.equal(signatureBytes.subarray(0, 2).toString('ascii'), 'ED',
    'Only prehashed minisign signatures are supported');
  assert.ok(signatureBytes.subarray(2, 10).equals(keyBytes.subarray(2, 10)),
    'Updater signature key ID does not match the pinned public key');

  const rawSignature = signatureBytes.subarray(10);
  const key = createPublicKey({
    key: Buffer.concat([Buffer.from('302a300506032b6570032100', 'hex'), keyBytes.subarray(10)]),
    format: 'der', type: 'spki',
  });
  const prehash = createHash('blake2b512').update(installerBytes).digest();
  assert.ok(verifyEd25519(null, prehash, key, rawSignature),
    'Signed updater package has an invalid minisign signature');
  const trustedText = Buffer.from(lines[2].slice('trusted comment: '.length), 'utf8');
  assert.ok(verifyEd25519(null, Buffer.concat([rawSignature, trustedText]), key, commentSignature),
    'Minisign trusted comment has an invalid signature');
  return trustedText.toString('utf8');
}

export function verifyUpdaterArtifact(installerPath, signaturePath, configPath = 'src-tauri/tauri.conf.json') {
  const config = JSON.parse(readFileSync(configPath, 'utf8'));
  const pubkey = config.plugins?.updater?.pubkey;
  assert.equal(typeof pubkey, 'string', 'Updater public key not configured');
  const comment = verifyUpdaterSignature(
    readFileSync(installerPath), readFileSync(signaturePath, 'utf8'), pubkey);
  const match = comment.match(/(?:^|\t)file:([^\t]+)/);
  if (match) assert.equal(match[1], basename(installerPath),
    'Signed update refers to a different installer');
  return comment;
}

if (process.argv[1] && resolve(fileURLToPath(import.meta.url)) === resolve(process.argv[1])) {
  const [, , installer, signature, config] = process.argv;
  assert.ok(installer && signature, 'Usage: verify-updater-signature.mjs <installer> <signature> [config]');
  const comment = verifyUpdaterArtifact(installer, signature, config);
  console.log('PASS updater minisign package signature and signed comment:', comment);
}
