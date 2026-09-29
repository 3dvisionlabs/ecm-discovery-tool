// FDP types, constants and helpers for the mock (scripts/fdp-mock.ts).
// Deliberately a separate TypeScript implementation of the spec
// (discovery-protocol.md): the mock checks the app's Rust implementation
// (protocol/) against it.

// FDP port (spec §2.1). Override with FDP_PORT.
export const FDP_DEFAULT_PORT = 27270;
export const FDP_PROTO = 'fdp';
export const FDP_MAX_PAYLOAD = 1400;

// Versioning (spec §3.2): `discover`/`announce` are frozen at version 1 so any
// client can find any device. `announce.versions` lists the versions the
// device speaks for targeted requests; the client uses the highest common one.
export const FDP_DISCOVERY_VERSION = 1;
export const FDP_CLIENT_VERSIONS = [1];
export const FDP_ENC_ALG = 'x25519-hkdf-sha256-aes256gcm';

/** Highest version both sides speak, or null. Devices without `versions` speak only v1. */
export function negotiateVersion(deviceVersions: number[] | undefined): number | null {
  const device = Array.isArray(deviceVersions) ? deviceVersions : [1];
  const common = FDP_CLIENT_VERSIONS.filter(v => device.includes(v));
  return common.length ? Math.max(...common) : null;
}

export function hkdfInfo(version: number): string {
  return `fdp/${version} set_ip`;
}

export type NetMode = 'dhcp' | 'static';

export interface StaticAddress {
  address: string; // CIDR, e.g. "192.168.1.100/24"
  gateway: string | null;
}

// Current network state as reported in `announce` (spec §4.1)
export interface NetState {
  interface: string;
  mode: NetMode;
  address: string | null;
  gateway: string | null;
  dns: string[];
  fallback: StaticAddress | null;
}

// Requested configuration in `set_ip` (spec §4.3). `default`: the device
// applies its own factory network configuration; other fields are ignored.
export interface NetConfig {
  mode: NetMode | 'default';
  address?: string;
  gateway?: string | null;
  dns?: string[];
  fallback?: StaticAddress | null;
}

export interface Target {
  serial: string;
  mac: string;
}

export interface DeviceInfo {
  serial: string;
  mac: string;
  vendor: string;
  model: string;
  hostname: string;
  fw_version: string;
  web: { scheme: 'https' | 'http'; port: number };
  key_fp: string;
}

interface Header<T extends string> {
  proto: typeof FDP_PROTO;
  v: number;
  type: T;
  id: string;
}

export type DiscoverMsg = Header<'discover'>;

export interface AnnounceMsg extends Header<'announce'> {
  device: DeviceInfo;
  net: NetState;
  caps: string[];
  set_ip: 'enabled' | 'disabled';
  // Protocol versions for targeted requests; missing = [1] (spec §3.2)
  versions?: number[];
}

export interface ChallengeRequestMsg extends Header<'challenge_request'> {
  target: Target;
}

export interface ChallengeMsg extends Header<'challenge'> {
  challenge: string;
  key: { alg: 'x25519'; pub: string };
}

export interface SetIpMsg extends Header<'set_ip'> {
  target: Target;
  challenge: string;
  enc: { alg: string; epk: string; nonce: string; ct: string };
}

export type SetIpStatus =
  | 'ok' | 'auth_failed' | 'bad_request' | 'invalid_config'
  | 'disabled' | 'locked' | 'error';

export interface SetIpResultMsg extends Header<'set_ip_result'> {
  status: SetIpStatus;
  message?: string;
  net?: NetState;
}

export interface IdentifyMsg extends Header<'identify'> {
  target: Target;
  duration_s: number;
}

export interface IdentifyResultMsg extends Header<'identify_result'> {
  status: 'ok' | 'error';
  message?: string;
}

// Answer to a targeted request with a version the device does not speak
export interface VersionErrorMsg extends Header<'version_error'> {
  versions: number[];
}

export type FdpMessage =
  | DiscoverMsg | AnnounceMsg | ChallengeRequestMsg | ChallengeMsg
  | SetIpMsg | SetIpResultMsg | IdentifyMsg | IdentifyResultMsg | VersionErrorMsg;

// Plaintext inside `set_ip.enc` (spec §4.3)
export interface SetIpPayload {
  user: string;
  password: string;
  config: NetConfig;
}

/** AAD for the `set_ip` encryption (spec §5.2). */
export function setIpAad(version: number, target: Target, id: string, challenge: string, epkHex: string): string {
  return [`fdp/${version}`, 'set_ip', target.serial, target.mac, id, challenge, epkHex].join('\n');
}

/** Parse and minimally validate an incoming datagram. The caller checks `v`. */
export function parseMessage(data: Uint8Array | string): FdpMessage | null {
  const text = typeof data === 'string' ? data : new TextDecoder().decode(data);
  if (text.length > FDP_MAX_PAYLOAD) return null;
  try {
    const msg = JSON.parse(text);
    if (!msg || msg.proto !== FDP_PROTO || !Number.isInteger(msg.v) || msg.v < 1) return null;
    if (typeof msg.type !== 'string' || typeof msg.id !== 'string') return null;
    return msg as FdpMessage;
  } catch {
    return null;
  }
}

// --- IPv4 helpers ---

export function ipToInt(ip: string): number | null {
  const m = /^(\d{1,3})\.(\d{1,3})\.(\d{1,3})\.(\d{1,3})$/.exec(ip);
  if (!m) return null;
  const parts = m.slice(1).map(Number);
  if (parts.some(p => p > 255)) return null;
  return ((parts[0] << 24) | (parts[1] << 16) | (parts[2] << 8) | parts[3]) >>> 0;
}

export function intToIp(n: number): string {
  return [n >>> 24, (n >>> 16) & 255, (n >>> 8) & 255, n & 255].join('.');
}

export function prefixMask(prefix: number): number {
  return prefix === 0 ? 0 : (0xffffffff << (32 - prefix)) >>> 0;
}

export interface Cidr {
  ip: string;
  prefix: number;
  ipInt: number;
  network: number;
  broadcast: number;
}

export function parseCidr(cidr: string): Cidr | null {
  const [ip, prefixStr, ...rest] = cidr.trim().split('/');
  if (rest.length || prefixStr === undefined || !/^\d{1,2}$/.test(prefixStr)) return null;
  const prefix = Number(prefixStr);
  const ipInt = ipToInt(ip);
  if (ipInt === null || prefix > 32) return null;
  const mask = prefixMask(prefix);
  const network = (ipInt & mask) >>> 0;
  return { ip, prefix, ipInt, network, broadcast: (network | ~mask) >>> 0 };
}

/** True if `ip` lies within the subnet given as CIDR. */
export function inSubnet(ip: string, cidr: string): boolean {
  const c = parseCidr(cidr);
  const n = ipToInt(ip);
  if (!c || n === null) return false;
  return ((n & prefixMask(c.prefix)) >>> 0) === c.network;
}

export function stripPrefix(cidr: string | null): string | null {
  return cidr ? cidr.split('/')[0] : null;
}

function isUnicast(n: number): boolean {
  const first = n >>> 24;
  return first !== 0 && first !== 127 && first < 224;
}

// --- Configuration validation (spec §4.3.1) ---

function validateAddress(cidr: string | undefined, label: string): string | null {
  if (!cidr) return `${label} is required`;
  const c = parseCidr(cidr);
  if (!c) return `${label} must be in the form a.b.c.d/prefix`;
  if (c.prefix < 1 || c.prefix > 30) return `${label}: prefix must be between 1 and 30`;
  if (!isUnicast(c.ipInt)) return `${label} must be a unicast address`;
  if (c.ipInt === c.network || c.ipInt === c.broadcast) {
    return `${label} must not be the network or broadcast address`;
  }
  return null;
}

function validateGateway(gateway: string | null | undefined, cidr: string, label: string): string | null {
  if (!gateway) return null;
  const g = ipToInt(gateway);
  if (g === null || !isUnicast(g)) return `${label} is not a valid address`;
  if (!inSubnet(gateway, cidr)) return `${label} must be within ${cidr}`;
  if (gateway === stripPrefix(cidr)) return `${label} must differ from the device address`;
  return null;
}

/** Returns an error message, or null if the configuration is valid. */
export function validateConfig(config: NetConfig): string | null {
  if (config.mode === 'default') return null;
  if (config.mode !== 'static' && config.mode !== 'dhcp') return 'Mode must be static, DHCP or default';

  if (config.mode === 'static') {
    const err = validateAddress(config.address, 'Address')
      ?? validateGateway(config.gateway, config.address!, 'Gateway');
    if (err) return err;
  } else if (config.fallback) {
    const err = validateAddress(config.fallback.address, 'Fallback address')
      ?? validateGateway(config.fallback.gateway, config.fallback.address, 'Fallback gateway');
    if (err) return err;
  }

  const dns = config.dns ?? [];
  if (dns.length > 3) return 'At most 3 DNS servers are allowed';
  for (const server of dns) {
    const n = ipToInt(server);
    if (n === null || !isUnicast(n)) return `DNS server ${server} is not a valid address`;
  }
  return null;
}

// --- Suggested configuration for the "Change IP" dialog (spec §8.4) ---

function isLinkLocal(ip: string): boolean {
  return inSubnet(ip, '169.254.0.0/16');
}

function randomInt(min: number, max: number): number {
  return min + Math.floor(Math.random() * (max - min + 1));
}

/**
 * Propose a configuration that makes the device reachable from the client
 * interface `localCidr`. The user can edit it; we cannot check whether the
 * address is free.
 */
export function suggestConfig(localCidr: string, current: NetState): NetConfig {
  const local = parseCidr(localCidr);

  // A static camera in a normal (non-APIPA) network: DHCP is the likely fix.
  if (local && !isLinkLocal(local.ip) && current.mode === 'static') {
    return { mode: 'dhcp', fallback: current.fallback, dns: [] };
  }

  if (!local || local.prefix > 30) {
    return { mode: 'static', address: '', gateway: null, dns: [] };
  }

  if (isLinkLocal(local.ip)) {
    // RFC 3927: 169.254.1.0 – 169.254.254.255
    let address: string;
    do {
      address = `169.254.${randomInt(1, 254)}.${randomInt(1, 254)}`;
    } while (address === local.ip);
    return { mode: 'static', address: `${address}/16`, gateway: null, dns: [] };
  }

  // Pick an address near the top of the local subnet, avoiding the PC's own address.
  const hosts = local.broadcast - local.network - 1;
  let candidate: number;
  do {
    candidate = local.broadcast - 1 - randomInt(0, Math.min(19, hosts - 1));
  } while (candidate === local.ipInt && hosts > 1);
  return { mode: 'static', address: `${intToIp(candidate)}/${local.prefix}`, gateway: null, dns: [] };
}
