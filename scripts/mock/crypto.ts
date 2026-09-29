// FDP cryptography for the mock (spec §5): X25519 + HKDF-SHA256 + AES-256-GCM,
// with Node's built-in crypto module (independent of the Rust implementation).
import * as crypto from 'crypto';
import { FDP_ENC_ALG, hkdfInfo } from './fdp';

// DER prefixes for wrapping raw 32-byte X25519 keys
const SPKI_PREFIX = Buffer.from('302a300506032b656e032100', 'hex');
const PKCS8_PREFIX = Buffer.from('302e020100300506032b656e04220420', 'hex');

export interface RawKeyPair {
  priv: Buffer;
  pub: Buffer;
}

export function publicKeyFromRaw(raw: Buffer): crypto.KeyObject {
  if (raw.length !== 32) throw new Error('X25519 public key must be 32 bytes');
  return crypto.createPublicKey({ key: Buffer.concat([SPKI_PREFIX, raw]), format: 'der', type: 'spki' });
}

export function privateKeyFromRaw(raw: Buffer): crypto.KeyObject {
  if (raw.length !== 32) throw new Error('X25519 private key must be 32 bytes');
  return crypto.createPrivateKey({ key: Buffer.concat([PKCS8_PREFIX, raw]), format: 'der', type: 'pkcs8' });
}

function rawPublic(key: crypto.KeyObject): Buffer {
  return (crypto.createPublicKey(key).export({ format: 'der', type: 'spki' }) as Buffer).subarray(-32);
}

function rawPrivate(key: crypto.KeyObject): Buffer {
  return (key.export({ format: 'der', type: 'pkcs8' }) as Buffer).subarray(-32);
}

export function generateKeyPair(): RawKeyPair {
  const { privateKey } = crypto.generateKeyPairSync('x25519');
  return { priv: rawPrivate(privateKey), pub: rawPublic(privateKey) };
}

export function keyPairFromPrivate(priv: Buffer): RawKeyPair {
  const key = privateKeyFromRaw(priv);
  return { priv: rawPrivate(key), pub: rawPublic(key) };
}

/** OpenSSH-style fingerprint: "SHA256:" + unpadded base64 (spec §5.1). */
export function fingerprint(pub: Buffer): string {
  return 'SHA256:' + crypto.createHash('sha256').update(pub).digest('base64').replace(/=+$/, '');
}

function deriveKey(version: number, shared: Buffer, epk: Buffer, devicePub: Buffer): Buffer {
  if (shared.every(b => b === 0)) throw new Error('X25519 produced an all-zero shared secret');
  return Buffer.from(crypto.hkdfSync('sha256', shared, Buffer.concat([epk, devicePub]), hkdfInfo(version), 32));
}

export interface Sealed {
  alg: string;
  epk: string;
  nonce: string;
  ct: string;
}

export interface SealOptions {
  // Fixed values for test vectors only
  ephemeralPriv?: Buffer;
  nonce?: Buffer;
}

/**
 * Encrypt `plaintext` for the device public key. `aadFor` builds the AAD from
 * the ephemeral public key (hex), because the AAD includes it. The protocol
 * version is bound into the key derivation.
 */
export function seal(
  version: number,
  devicePub: Buffer,
  plaintext: string,
  aadFor: (epkHex: string) => string,
  opts: SealOptions = {},
): Sealed {
  const eph = opts.ephemeralPriv ? keyPairFromPrivate(opts.ephemeralPriv) : generateKeyPair();
  const shared = crypto.diffieHellman({
    privateKey: privateKeyFromRaw(eph.priv),
    publicKey: publicKeyFromRaw(devicePub),
  });
  const key = deriveKey(version, shared, eph.pub, devicePub);
  const nonce = opts.nonce ?? crypto.randomBytes(12);
  const epkHex = eph.pub.toString('hex');

  const cipher = crypto.createCipheriv('aes-256-gcm', key, nonce, { authTagLength: 16 });
  cipher.setAAD(Buffer.from(aadFor(epkHex), 'utf8'));
  const ct = Buffer.concat([cipher.update(plaintext, 'utf8'), cipher.final(), cipher.getAuthTag()]);

  return { alg: FDP_ENC_ALG, epk: epkHex, nonce: nonce.toString('hex'), ct: ct.toString('hex') };
}

/** Decrypt a sealed payload with the device key pair. Throws on any failure. */
export function open(version: number, device: RawKeyPair, sealed: Sealed, aad: string): string {
  if (sealed.alg !== FDP_ENC_ALG) throw new Error(`unsupported alg ${sealed.alg}`);
  const epk = Buffer.from(sealed.epk, 'hex');
  const nonce = Buffer.from(sealed.nonce, 'hex');
  const ct = Buffer.from(sealed.ct, 'hex');
  if (epk.length !== 32 || nonce.length !== 12 || ct.length < 16) throw new Error('malformed payload');

  const shared = crypto.diffieHellman({
    privateKey: privateKeyFromRaw(device.priv),
    publicKey: publicKeyFromRaw(epk),
  });
  const key = deriveKey(version, shared, epk, device.pub);

  const decipher = crypto.createDecipheriv('aes-256-gcm', key, nonce, { authTagLength: 16 });
  decipher.setAAD(Buffer.from(aad, 'utf8'));
  decipher.setAuthTag(ct.subarray(ct.length - 16));
  return Buffer.concat([decipher.update(ct.subarray(0, ct.length - 16)), decipher.final()]).toString('utf8');
}

export function randomHex(bytes: number): string {
  return crypto.randomBytes(bytes).toString('hex');
}
