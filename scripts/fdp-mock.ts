// FDP mock responder: simulates several cameras that implement
// discovery-protocol.md, so the app can be tested without real hardware.
//
// Run:   npm run mock:fdp -- [--password 3dvl] [--web-port 18080] [--new-keys]
// Then:  npm start
//
// Unlike real firmware, the mock uses a normal UDP socket instead of raw
// layer-2 sockets. It answers from this machine's address, which the client
// does not check. It cannot take over the addresses set via "Change IP", so
// after a change the camera shows the new address but its web UI stays
// unreachable ("not responding" or "different subnet" in the app).
import * as dgram from 'dgram';
import * as fs from 'fs';
import * as http from 'http';
import * as os from 'os';
import * as path from 'path';
import {
  AnnounceMsg, FDP_DEFAULT_PORT, NetConfig, FDP_DISCOVERY_VERSION, FdpMessage, NetState, SetIpPayload, SetIpStatus,
  inSubnet, parseMessage, setIpAad, validateConfig,
} from './mock/fdp';
import { RawKeyPair, fingerprint, generateKeyPair, keyPairFromPrivate, open, randomHex } from './mock/crypto';

// --- Options ---

function arg(name: string): string | undefined {
  const i = process.argv.indexOf(`--${name}`);
  return i >= 0 ? process.argv[i + 1] : undefined;
}

const PORT = Number(arg('port') ?? process.env.FDP_PORT) || FDP_DEFAULT_PORT;
const PASSWORD = arg('password') ?? '3dvl';
const WEB_PORT = Number(arg('web-port')) || 18080;
const NEW_KEYS = process.argv.includes('--new-keys');
const KEYS_FILE = path.join(os.tmpdir(), 'fdp-mock-keys.json');

const CHALLENGE_TTL = 30_000;
const MAX_CHALLENGES = 8;
const LOCKOUT_ATTEMPTS = 5;
const LOCKOUT_WINDOW = 5 * 60_000;
const DEDUP_WINDOW = 5_000;
const MAX_ANNOUNCES_PER_SECOND = 20; // per simulated camera (spec §2.5)

// --- Simulated cameras ---

interface MockCamera {
  serial: string;
  mac: string;
  hostname: string;
  model: string;
  fw: string;
  caps: string[];
  setIp: 'enabled' | 'disabled';
  net: NetState;
  // Announce this machine's own address, so the web UI is really reachable
  onHost: boolean;
  // Protocol versions for targeted requests (spec §3.2)
  versions: number[];
  // Simulates firmware from before `versions` existed: no `versions` in
  // announce, silently ignores requests with an unknown version
  legacy: boolean;
  web: { scheme: 'https' | 'http'; port: number };
  key: RawKeyPair;
  challenges: Map<string, number>;
  failures: number[];
  lockedUntil: number;
  lastIdentify: number;
  announcesThisSecond: number;
}

type MockCameraConfig = Omit<MockCamera,
  'key' | 'challenges' | 'failures' | 'lockedUntil' | 'lastIdentify' | 'announcesThisSecond' | 'versions' | 'legacy'>
  & Partial<Pick<MockCamera, 'versions' | 'legacy'>>;

function camera(c: MockCameraConfig): MockCamera {
  return {
    versions: [1], legacy: false, ...c,
    key: null as unknown as RawKeyPair, challenges: new Map(), failures: [],
    lockedUntil: 0, lastIdentify: 0, announcesThisSecond: 0,
  };
}

const cameras: MockCamera[] = [
  camera({
    // Factory settings on a direct cable: DHCP failed, static fallback active
    serial: '244260001238', mac: '00:1b:c5:0a:0b:0c', hostname: 'ecm-244260001238', model: 'C7', fw: '3.1.0',
    caps: ['set_ip', 'identify'], setIp: 'enabled', onHost: false, web: { scheme: 'https', port: 443 },
    net: {
      interface: 'eth0', mode: 'dhcp', address: '192.168.1.100/24', gateway: '192.168.1.1', dns: [],
      fallback: { address: '192.168.1.100/24', gateway: '192.168.1.1' },
    },
  }),
  camera({
    // Renamed camera in the PC's subnet with a reachable (mock) web UI
    serial: '244260001377', mac: '00:1b:c5:0a:13:77', hostname: 'packaging-1', model: 'C7', fw: '3.1.0',
    caps: ['set_ip', 'identify'], setIp: 'enabled', onHost: true, web: { scheme: 'http', port: WEB_PORT },
    net: { interface: 'eth0', mode: 'static', address: null, gateway: null, dns: [], fallback: null },
  }),
  camera({
    // IP change disabled in the web UI
    serial: '232250000988', mac: '00:1b:c5:09:88:01', hostname: 'ecm-232250000988', model: 'C7', fw: '3.1.0',
    caps: ['set_ip', 'identify'], setIp: 'disabled', onHost: false, web: { scheme: 'https', port: 443 },
    net: { interface: 'eth0', mode: 'static', address: '10.10.0.5/16', gateway: '10.10.0.1', dns: ['10.10.0.1'], fallback: null },
  }),
  camera({
    // Hardware without identify
    serial: '250110000042', mac: '00:1b:c5:00:00:42', hostname: 'ecm-250110000042', model: 'C5', fw: '3.1.0',
    caps: ['set_ip'], setIp: 'enabled', onHost: false, web: { scheme: 'https', port: 443 },
    net: { interface: 'eth0', mode: 'static', address: '192.168.0.23/24', gateway: '192.168.0.1', dns: [], fallback: null },
  }),
  camera({
    // Hypothetical newer firmware that only speaks protocol v2: the app shows
    // it as incompatible ("update this app")
    serial: '231150000077', mac: '00:1b:c5:00:00:77', hostname: 'ecm-231150000077', model: 'ECM',
    fw: '6.0.0', caps: ['set_ip', 'identify'], setIp: 'enabled', onHost: false,
    web: { scheme: 'https', port: 443 }, versions: [2],
    net: { interface: 'eth0', mode: 'static', address: '172.31.5.20/24', gateway: null, dns: [], fallback: null },
  }),
];

// Keys persist across restarts (like the device key in /data); --new-keys simulates a factory reset
function loadKeys(): void {
  let stored: Record<string, string> = {};
  if (!NEW_KEYS) {
    try {
      stored = JSON.parse(fs.readFileSync(KEYS_FILE, 'utf8'));
    } catch {
      stored = {};
    }
  }
  for (const cam of cameras) {
    cam.key = stored[cam.serial] ? keyPairFromPrivate(Buffer.from(stored[cam.serial], 'hex')) : generateKeyPair();
    stored[cam.serial] = cam.key.priv.toString('hex');
  }
  fs.writeFileSync(KEYS_FILE, JSON.stringify(stored, null, 2), { mode: 0o600 });
}

// --- Helpers ---

function log(tag: string, text: string): void {
  const time = new Date().toTimeString().slice(0, 8);
  console.log(`${time} [${tag}] ${text}`);
}

/** This machine's address in the requester's subnet, for cameras simulated "on host". */
function hostCidrFor(requester: string): string {
  let fallback: string | null = null;
  for (const addrs of Object.values(os.networkInterfaces())) {
    for (const a of addrs ?? []) {
      if (a.family !== 'IPv4' || !a.cidr) continue;
      if (inSubnet(requester, a.cidr)) return a.cidr;
      if (!a.internal && !fallback) fallback = a.cidr;
    }
  }
  return fallback ?? `${requester}/32`;
}

function currentNet(cam: MockCamera, requester: string): NetState {
  if (!cam.onHost) return cam.net;
  return { ...cam.net, address: hostCidrFor(requester) };
}

function isLocked(cam: MockCamera): boolean {
  return Date.now() < cam.lockedUntil;
}

function countFailure(cam: MockCamera): void {
  const now = Date.now();
  cam.failures = cam.failures.filter(t => now - t < LOCKOUT_WINDOW);
  cam.failures.push(now);
  if (cam.failures.length >= LOCKOUT_ATTEMPTS) {
    cam.lockedUntil = now + LOCKOUT_WINDOW;
    cam.failures = [];
    log('lockout', `${cam.serial} locked for 5 minutes`);
  }
}

// --- UDP server ---

const sock = dgram.createSocket({ type: 'udp4', reuseAddr: true });
const seen = new Map<string, number>();
setInterval(() => {
  for (const cam of cameras) cam.announcesThisSecond = 0;
  const now = Date.now();
  for (const [key, t] of seen) if (now - t > DEDUP_WINDOW) seen.delete(key);
}, 1000);

function reply(rinfo: dgram.RemoteInfo, msg: object): void {
  const buf = Buffer.from(JSON.stringify(msg), 'utf8');
  sock.send(buf, rinfo.port, rinfo.address);
}

function header(type: string, id: string, v = FDP_DISCOVERY_VERSION) {
  return { proto: 'fdp', v, type, id };
}

function findTarget(msg: { target?: { serial: string; mac: string } }): MockCamera | undefined {
  return cameras.find(c => c.serial === msg.target?.serial && c.mac === msg.target?.mac);
}

function announce(cam: MockCamera, id: string, requester: string): AnnounceMsg {
  return {
    ...header('announce', id) as AnnounceMsg,
    device: {
      serial: cam.serial, mac: cam.mac, vendor: '3dvisionlabs', model: cam.model, hostname: cam.hostname,
      fw_version: cam.fw, web: cam.web, key_fp: fingerprint(cam.key.pub),
    },
    net: currentNet(cam, requester),
    caps: cam.caps,
    set_ip: cam.setIp,
    ...(cam.legacy ? {} : { versions: cam.versions }),
  };
}

function setIpResult(rinfo: dgram.RemoteInfo, req: FdpMessage, status: SetIpStatus, message = '', net?: NetState): void {
  reply(rinfo, { ...header('set_ip_result', req.id, req.v), status, message, ...(net ? { net } : {}) });
}

function handle(msg: FdpMessage, rinfo: dgram.RemoteInfo): void {
  const from = `${rinfo.address}:${rinfo.port}`;

  // Targeted requests must use a version the camera speaks (spec §3.2).
  // `discover` is answered for any version.
  if (msg.type !== 'discover') {
    const cam = findTarget(msg as { target?: { serial: string; mac: string } });
    if (cam && !cam.versions.includes(msg.v)) {
      log('version', `${cam.serial}: ${msg.type} with v${msg.v}, speaks ${cam.versions.map(v => `v${v}`).join(', ')}`);
      if (!cam.legacy) reply(rinfo, { ...header('version_error', msg.id, msg.v), versions: cam.versions });
      return;
    }
  }

  switch (msg.type) {
    case 'discover': {
      log('discover', `from ${from}`);
      for (const cam of cameras) {
        if (cam.announcesThisSecond >= MAX_ANNOUNCES_PER_SECOND) continue;
        cam.announcesThisSecond++;
        setTimeout(() => reply(rinfo, announce(cam, msg.id, rinfo.address)), Math.random() * 250);
      }
      return;
    }

    case 'challenge_request': {
      const cam = findTarget(msg);
      if (!cam) return;
      if (cam.setIp === 'disabled') {
        log('challenge', `${cam.serial} refused: disabled`);
        return setIpResult(rinfo, msg, 'disabled');
      }
      if (isLocked(cam)) return setIpResult(rinfo, msg, 'locked');

      const challenge = randomHex(16);
      cam.challenges.set(challenge, Date.now());
      while (cam.challenges.size > MAX_CHALLENGES) {
        cam.challenges.delete(cam.challenges.keys().next().value!);
      }
      log('challenge', `${cam.serial} → ${challenge.slice(0, 8)}… (key ${fingerprint(cam.key.pub).slice(0, 18)}…)`);
      reply(rinfo, { ...header('challenge', msg.id, msg.v), challenge, key: { alg: 'x25519', pub: cam.key.pub.toString('hex') } });
      return;
    }

    case 'set_ip': {
      const cam = findTarget(msg);
      if (!cam) return;
      if (cam.setIp === 'disabled') return setIpResult(rinfo, msg, 'disabled');
      if (isLocked(cam)) return setIpResult(rinfo, msg, 'locked');

      // Invalidate the challenge whatever the outcome (spec §4.3)
      const issued = cam.challenges.get(msg.challenge);
      cam.challenges.delete(msg.challenge);
      if (!issued || Date.now() - issued > CHALLENGE_TTL) {
        countFailure(cam);
        log('set_ip', `${cam.serial} auth_failed: unknown or expired challenge`);
        return setIpResult(rinfo, msg, 'auth_failed');
      }

      let payload: SetIpPayload;
      try {
        payload = JSON.parse(open(msg.v, cam.key, msg.enc, setIpAad(msg.v, msg.target, msg.id, msg.challenge, msg.enc.epk)));
      } catch (err) {
        countFailure(cam);
        log('set_ip', `${cam.serial} bad_request: ${(err as Error).message}`);
        return setIpResult(rinfo, msg, 'bad_request');
      }

      const configError = validateConfig(payload.config);
      if (configError) {
        log('set_ip', `${cam.serial} invalid_config: ${configError}`);
        return setIpResult(rinfo, msg, 'invalid_config', configError);
      }

      if (payload.user !== 'admin' || payload.password !== PASSWORD) {
        countFailure(cam);
        log('set_ip', `${cam.serial} auth_failed: wrong credentials for "${payload.user}"`);
        return setIpResult(rinfo, msg, 'auth_failed');
      }

      // `default`: like `network.v1.Network/ResetToDefault` on the device (spec §5.3),
      // which resets all interfaces to the factory settings (DHCP with static
      // fallback, spec §1). The mock has one interface per camera, and the
      // result reports the receiving one.
      const factoryDefault = payload.config.mode === 'default';
      const c: NetConfig = factoryDefault
        ? { mode: 'dhcp', fallback: { address: '192.168.1.100/24', gateway: '192.168.1.1' }, dns: [] }
        : payload.config;
      const iface = cam.net.interface;
      cam.onHost = false;
      cam.net = c.mode === 'static'
        ? { interface: iface, mode: 'static', address: c.address!, gateway: c.gateway ?? null, dns: c.dns ?? [], fallback: null }
        // No DHCP server in the simulation: the fallback becomes active
        : {
          interface: iface, mode: 'dhcp', address: c.fallback?.address ?? null, gateway: c.fallback?.gateway ?? null,
          dns: c.dns ?? [], fallback: c.fallback ?? null,
        };
      log('set_ip', `${cam.serial} ok${factoryDefault ? ' (reset to default)' : ''} → ${cam.net.mode} ${cam.net.address ?? '(no address)'}`);
      return setIpResult(rinfo, msg, 'ok', '', cam.net);
    }

    case 'identify': {
      const cam = findTarget(msg);
      if (!cam || !cam.caps.includes('identify')) return;
      // At most one start per 5 s; within that window answer ok without restarting (spec §4.4)
      if (Date.now() - cam.lastIdentify < 5_000) {
        log('identify', `${cam.serial} already flashing, not restarted`);
        return reply(rinfo, { ...header('identify_result', msg.id, msg.v), status: 'ok' });
      }
      cam.lastIdentify = Date.now();
      const duration = Math.min(60, Math.max(1, msg.duration_s || 10));
      log('identify', `${cam.serial} status LEDs flashing alternately for ${duration} s`);
      return reply(rinfo, { ...header('identify_result', msg.id, msg.v), status: 'ok' });
    }

    default:
      return; // responses from other devices, unknown types
  }
}

sock.on('message', (data, rinfo) => {
  const msg = parseMessage(data);
  if (!msg || rinfo.address === '0.0.0.0') return;

  // The client may send the same request via two broadcast addresses (spec §2.4)
  const key = `${rinfo.address}:${rinfo.port}:${msg.id}`;
  if (seen.has(key)) return;
  seen.set(key, Date.now());

  handle(msg, rinfo);
});

sock.on('error', (err) => {
  console.error(`UDP error: ${err.message}`);
  process.exit(1);
});

// --- Minimal web UI for the "on host" camera ---

const web = http.createServer((_req, res) => {
  res.writeHead(200, { 'Content-Type': 'text/html; charset=utf-8' });
  res.end('<!doctype html><title>FDP mock</title><h1>FDP mock camera</h1><p>packaging-1 (S/N 244260001377)</p>');
});
web.on('error', (err) => console.error(`Web server on port ${WEB_PORT}: ${err.message}`));

// --- Start ---

loadKeys();
sock.bind(PORT, () => {
  sock.setBroadcast(true);
  web.listen(WEB_PORT);
  console.log(`FDP mock listening on UDP ${PORT}, mock web UI on http port ${WEB_PORT}`);
  console.log(`Credentials: admin / ${PASSWORD}   Keys: ${KEYS_FILE}${NEW_KEYS ? ' (regenerated)' : ''}\n`);
  for (const cam of cameras) {
    const address = cam.onHost ? '(this PC)' : cam.net.address;
    const flags = [cam.setIp === 'disabled' && 'set_ip disabled', !cam.caps.includes('identify') && 'no identify']
      .filter(Boolean).join(', ');
    console.log(`  ${cam.hostname.padEnd(18)} S/N ${cam.serial}  ${String(address).padEnd(18)} ${fingerprint(cam.key.pub)}${flags ? `  [${flags}]` : ''}`);
  }
  console.log('\nTips: --new-keys simulates a factory reset (key warning in the app).');
  console.log('      Start the app with FDP_LOOPBACK=1 to also scan via 127.0.0.1 (no network needed).\n');
});
