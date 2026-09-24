import type { NetConfig, NetState, SetIpStatus } from './ecmdp';

// online:       web UI reachable (TCP connect succeeds)
// other-subnet: answers ECMDP, but its address is outside the PC's subnet
// unreachable:  answers ECMDP in the PC's subnet, but the web UI does not respond
// offline:      no longer answering
export type CameraStatus = 'online' | 'other-subnet' | 'unreachable' | 'offline';

export type KeyTrust = 'new' | 'match' | 'mismatch';

// Present for cameras found via ECMDP (spec discovery-protocol.md)
export interface EcmdpInfo {
  serial: string;
  mac: string;
  vendor: string;
  model: string;
  fwVersion: string;
  net: NetState;
  caps: string[];
  setIp: 'enabled' | 'disabled';
  keyFp: string;
  keyTrust: KeyTrust;
  // Protocol versions the camera speaks, and the one used for requests
  // (null: no common version, the app or firmware needs an update)
  versions: number[];
  version: number | null;
  // PC interface on which the camera answered
  localAddress: string;
  localCidr: string;
}

export interface Camera {
  id: string;
  hostname: string;
  ip: string;
  port: number;
  scheme: 'https' | 'http';
  lastSeen: number;
  online: boolean;
  status: CameraStatus;
  ecmdp?: EcmdpInfo;
}

export interface PrepareSetIpResult {
  ok: boolean;
  error?: string;
  fingerprint?: string;
  trust?: KeyTrust;
  pinnedFingerprint?: string | null;
  suggestion?: NetConfig;
}

export interface SetIpRequest {
  id: string;
  config: NetConfig;
  user: string;
  password: string;
  acceptKey: boolean;
}

export interface SetIpResult {
  status: SetIpStatus | 'timeout' | 'untrusted' | 'key_changed' | 'unsupported_version';
  message?: string;
  net?: NetState;
}

export interface IdentifyResult {
  ok: boolean;
  message?: string;
}
