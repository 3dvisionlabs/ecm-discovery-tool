// ECMDP types and configuration validation for the UI. The protocol itself
// (messages, crypto, suggestion) lives in crates/ecmdp; keep `validateConfig`
// in sync with `validate_config` there, which the backend checks again.

export const ECMDP_CLIENT_VERSIONS = [1];

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

// Requested configuration in `set_ip` (spec §4.3)
export interface NetConfig {
  mode: NetMode;
  address?: string;
  gateway?: string | null;
  dns?: string[];
  fallback?: StaticAddress | null;
}

export type SetIpStatus =
  | 'ok' | 'auth_failed' | 'bad_request' | 'invalid_config'
  | 'disabled' | 'locked' | 'error';

// --- IPv4 helpers ---

export function ipToInt(ip: string): number | null {
  const m = /^(\d{1,3})\.(\d{1,3})\.(\d{1,3})\.(\d{1,3})$/.exec(ip);
  if (!m) return null;
  const parts = m.slice(1).map(Number);
  if (parts.some(p => p > 255)) return null;
  return ((parts[0] << 24) | (parts[1] << 16) | (parts[2] << 8) | parts[3]) >>> 0;
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
  if (config.mode !== 'static' && config.mode !== 'dhcp') return 'Mode must be static or DHCP';

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
