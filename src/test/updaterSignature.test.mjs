import { createHash, generateKeyPairSync, sign } from 'node:crypto';
import { describe, expect, it } from 'vitest';
import { verifyUpdaterSignature } from '../../scripts/verify-updater-signature.mjs';

function syntheticSignature(payload) {
  const { privateKey, publicKey } = generateKeyPairSync('ed25519');
  const rawKey = publicKey.export({ format: 'der', type: 'spki' }).subarray(-32);
  const keyId = Buffer.from('0011223344556677', 'hex');
  const publicRecord = Buffer.concat([Buffer.from('Ed'), keyId, rawKey]);
  const keyFile = `untrusted comment: test-only key\n${publicRecord.toString('base64')}\n`;
  const digest = createHash('blake2b512').update(payload).digest();
  const contentsSignature = sign(null, digest, privateKey);
  const signatureRecord = Buffer.concat([Buffer.from('ED'), keyId, contentsSignature]);
  const comment = 'timestamp:1792157124\tfile:MirroLeap_5.0.3_x64-setup.exe';
  const globalSignature = sign(null, Buffer.concat([
    contentsSignature, Buffer.from(comment, 'utf8')]), privateKey);
  const sigFile = [
    'untrusted comment: test-only signature',
    signatureRecord.toString('base64'),
    `trusted comment: ${comment}`,
    globalSignature.toString('base64'),
  ].join('\n') + '\n';
  return {
    key: Buffer.from(keyFile).toString('base64'),
    signature: Buffer.from(sigFile).toString('base64'),
    sigFile,
  };
}

describe('minisign updater verification', () => {
  it('accepts a correctly-signed prehashed update package', () => {
    const packageBytes = Buffer.from('official Windows installer fixture');
    const { key, signature } = syntheticSignature(packageBytes);
    expect(verifyUpdaterSignature(packageBytes, signature, key))
      .toContain('MirroLeap_5.0.3_x64-setup.exe');
  });

  it('rejects a modified package and tampered trusted comment', () => {
    const bytes = Buffer.from('official Windows installer fixture');
    const { key, signature, sigFile } = syntheticSignature(bytes);
    expect(() => verifyUpdaterSignature(Buffer.from('tampered installer'), signature, key))
      .toThrow(/signature/i);
    const tamperedComment = Buffer.from(
      sigFile.replace('timestamp:1792157124', 'timestamp:0000000000'))
      .toString('base64');
    expect(() => verifyUpdaterSignature(bytes, tamperedComment, key))
      .toThrow(/signature/i);
  });

  it('rejects a foreign signing key, malformed or legacy signatures', () => {
    const bytes = Buffer.from('some installer');
    const { signature, sigFile, key } = syntheticSignature(bytes);
    const foreign = syntheticSignature(bytes);
    expect(() => verifyUpdaterSignature(bytes, signature, foreign.key))
      .toThrow(/key ID|signature/i);
    expect(() => verifyUpdaterSignature(bytes, '!!!!', foreign.key))
      .toThrow(/base64/i);
    const parts = sigFile.split('\n');
    const record = Buffer.from(parts[1], 'base64');
    record[1] = 'd'.charCodeAt(0);
    parts[1] = record.toString('base64');
    const legacy = Buffer.from(parts.join('\n')).toString('base64');
    expect(() => verifyUpdaterSignature(bytes, legacy, key))
      .toThrow(/prehashed/i);
  });
});
