// Test vector from discovery-protocol.md §5.4, checked against the mock's
// Node.js crypto implementation (independent of protocol/).
// Run: npm run test:mock
import * as assert from 'assert';
import * as crypto from 'crypto';
import { fingerprint, keyPairFromPrivate, open, privateKeyFromRaw, publicKeyFromRaw, seal } from './crypto';
import { hkdfInfo, setIpAad } from './fdp';

const hex = (s: string) => Buffer.from(s, 'hex');

const device = keyPairFromPrivate(hex('a8' + 'ab'.repeat(30) + '6b'));
const ephemeralPriv = hex('c8' + 'cd'.repeat(30) + '4d');
const target = { serial: '244260001238', mac: '00:1b:c5:0a:0b:0c' };
const id = '5f2d3c4b1a09f8e7d6c5b4a392817060';
const challenge = 'a1b2c3d4e5f60718293a4b5c6d7e8f90';
const nonce = hex('000102030405060708090a0b');
const plaintext = '{"user":"admin","password":"3dvl","config":{"mode":"static","address":"169.254.10.20/16","gateway":null,"dns":[]}}';

const EPK = 'b5bea823d9c9ff576091c54b7c596c0ae296884f0e150290e88455d7fba6126f';
const SHARED = '235101b705734aae8d4c2d9d0f1baf90bbb2a8c233d831a80d43815bb47ead10';
const KEY = 'b6b3a069933df0174a91f35649b66e00d1dca31eaac4b0b3a17c64bff1966abf';
const AAD = `fdp/1\nset_ip\n244260001238\n00:1b:c5:0a:0b:0c\n${id}\n${challenge}\n${EPK}`;
const CT = '5ef0f83ee296f8735d5b8d04568555340437d6bed857ff47c59082266db115c23217a4fcfee6430aec6924067376441b385914d274889346f06d5a5a1d40b20ea7a58d12249cf7cb148e3cd58311918f0668209525a79a0b7d6fa701e62f43a1a8844a0d6091a8cdc18546ba83e7b5fa89ede083c6b2d30ea268044ed43d725e623d';

const checks: [string, () => void][] = [
  ['device public key', () =>
    assert.strictEqual(device.pub.toString('hex'), 'e3712d851a0e5d79b831c5e34ab22b41a198171de209b8b8faca23a11c624859')],
  ['fingerprint', () => assert.strictEqual(fingerprint(device.pub), 'SHA256:vXOB6rCNLWJnXCfHnpmCFX5xYXoJWIZR2Ku6xQPTmSM')],
  ['epk', () => assert.strictEqual(keyPairFromPrivate(ephemeralPriv).pub.toString('hex'), EPK)],
  ['shared and key', () => {
    const shared = crypto.diffieHellman({ privateKey: privateKeyFromRaw(ephemeralPriv), publicKey: publicKeyFromRaw(device.pub) });
    assert.strictEqual(shared.toString('hex'), SHARED);
    assert.strictEqual(hkdfInfo(1), 'fdp/1 set_ip');
    const key = Buffer.from(crypto.hkdfSync('sha256', shared, Buffer.concat([hex(EPK), device.pub]), hkdfInfo(1), 32));
    assert.strictEqual(key.toString('hex'), KEY);
  }],
  ['aad', () => assert.strictEqual(setIpAad(1, target, id, challenge, EPK), AAD)],
  ['ct', () => {
    const sealed = seal(1, device.pub, plaintext, epk => setIpAad(1, target, id, challenge, epk), { ephemeralPriv, nonce });
    assert.strictEqual(sealed.epk, EPK);
    assert.strictEqual(sealed.ct, CT);
  }],
  ['decrypt', () => {
    const sealed = { alg: 'x25519-hkdf-sha256-aes256gcm', epk: EPK, nonce: nonce.toString('hex'), ct: CT };
    assert.strictEqual(open(1, device, sealed, AAD), plaintext);
    assert.throws(() => open(1, device, sealed, AAD.replace('fdp/1', 'ecmdp/1')));
  }],
];

let failed = 0;
for (const [name, check] of checks) {
  try {
    check();
    console.log(`ok    ${name}`);
  } catch (err) {
    failed++;
    console.log(`FAIL  ${name}: ${(err as Error).message}`);
  }
}
process.exit(failed ? 1 : 0);
