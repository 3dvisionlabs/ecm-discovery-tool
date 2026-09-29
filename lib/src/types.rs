//! Types exchanged with the frontend (mirror of ui/src/shared/types.ts).

use fdp::{NetConfig, NetState};
use serde::{Deserialize, Serialize};

/// online:       web UI reachable (TCP connect succeeds)
/// other-subnet: answers FDP, but its address is outside the PC's subnet
/// unreachable:  answers FDP in the PC's subnet, but the web UI does not respond
/// offline:      no longer answering
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum CameraStatus {
    Online,
    OtherSubnet,
    Unreachable,
    Offline,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum KeyTrust {
    New,
    Match,
    Mismatch,
}

/// Present for cameras found via FDP (spec discovery-protocol.md).
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FdpInfo {
    pub serial: String,
    pub mac: String,
    pub vendor: String,
    pub model: String,
    pub fw_version: String,
    pub net: NetState,
    pub caps: Vec<String>,
    pub set_ip: String,
    pub key_fp: String,
    pub key_trust: KeyTrust,
    /// Protocol versions the camera speaks, and the one used for requests
    /// (None: no common version, the app or firmware needs an update)
    pub versions: Vec<u32>,
    pub version: Option<u32>,
    /// PC interface on which the camera answered
    pub local_address: String,
    pub local_cidr: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Camera {
    pub id: String,
    pub hostname: String,
    pub ip: String,
    pub port: u16,
    pub scheme: String,
    /// Unix time in ms
    pub last_seen: u64,
    pub online: bool,
    pub status: CameraStatus,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fdp: Option<FdpInfo>,
}

#[derive(Debug, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PrepareSetIpResult {
    pub ok: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fingerprint: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub trust: Option<KeyTrust>,
    pub pinned_fingerprint: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub suggestion: Option<NetConfig>,
}

impl PrepareSetIpResult {
    pub fn error(message: impl Into<String>) -> Self {
        PrepareSetIpResult { ok: false, error: Some(message.into()), ..Default::default() }
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SetIpRequest {
    pub id: String,
    pub config: NetConfig,
    pub user: String,
    pub password: String,
}

/// `status` is a device status (spec §4.3) or one of the client-side values
/// `timeout`, `untrusted`, `key_changed`, `unsupported_version`.
#[derive(Debug, Serialize)]
pub struct SetIpResult {
    pub status: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub net: Option<NetState>,
}

impl SetIpResult {
    pub fn new(status: &str, message: impl Into<String>) -> Self {
        SetIpResult { status: status.into(), message: Some(message.into()), net: None }
    }
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct IdentifyResult {
    pub ok: bool,
    pub message: String,
    /// How long the LED flashes (only on success)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub duration_s: Option<u32>,
}

impl IdentifyResult {
    pub fn failed(message: impl Into<String>) -> Self {
        IdentifyResult { ok: false, message: message.into(), duration_s: None }
    }
}

/// Unix time in ms, like JavaScript's `Date.now()`.
pub fn now_ms() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis() as u64).unwrap_or(0)
}
