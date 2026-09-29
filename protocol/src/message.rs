//! Message format (spec §3, §4).

use serde::{Deserialize, Serialize};

use crate::net::{NetConfig, NetState};
use crate::{MAX_PAYLOAD, PROTO};

/// Addresses a single device (spec §3.3).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Target {
    pub serial: String,
    pub mac: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WebInfo {
    pub scheme: String,
    pub port: u16,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeviceInfo {
    pub serial: String,
    pub mac: String,
    pub vendor: String,
    pub model: String,
    pub hostname: String,
    pub fw_version: String,
    pub web: WebInfo,
    pub key_fp: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeviceKey {
    pub alg: String,
    #[serde(rename = "pub")]
    pub public: String,
}

/// Encrypted `set_ip` payload (spec §5.2). All values hex.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Sealed {
    pub alg: String,
    pub epk: String,
    pub nonce: String,
    pub ct: String,
}

/// Message body, tagged by `type`. Unknown fields are ignored.
// Messages are short-lived; boxing `Announce` would only complicate matching.
#[allow(clippy::large_enum_variant)]
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Body {
    Discover {},
    Announce {
        device: DeviceInfo,
        net: NetState,
        caps: Vec<String>,
        set_ip: String,
        /// Protocol versions for targeted requests; missing = [1] (spec §3.2)
        #[serde(default, skip_serializing_if = "Option::is_none")]
        versions: Option<Vec<u32>>,
    },
    ChallengeRequest {
        target: Target,
    },
    Challenge {
        challenge: String,
        key: DeviceKey,
    },
    SetIp {
        target: Target,
        challenge: String,
        enc: Sealed,
    },
    SetIpResult {
        status: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        message: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        net: Option<NetState>,
    },
    Identify {
        target: Target,
        duration_s: u32,
    },
    IdentifyResult {
        status: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        message: Option<String>,
    },
    /// Answer to a targeted request with a version the device does not speak
    VersionError {
        versions: Vec<u32>,
    },
}

impl Body {
    pub fn type_name(&self) -> &'static str {
        match self {
            Body::Discover {} => "discover",
            Body::Announce { .. } => "announce",
            Body::ChallengeRequest { .. } => "challenge_request",
            Body::Challenge { .. } => "challenge",
            Body::SetIp { .. } => "set_ip",
            Body::SetIpResult { .. } => "set_ip_result",
            Body::Identify { .. } => "identify",
            Body::IdentifyResult { .. } => "identify_result",
            Body::VersionError { .. } => "version_error",
        }
    }
}

/// A complete datagram: common header (spec §3.1) plus body.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Message {
    pub proto: String,
    pub v: u32,
    pub id: String,
    #[serde(flatten)]
    pub body: Body,
}

impl Message {
    /// New message with a random id.
    pub fn new(version: u32, body: Body) -> Self {
        Message { proto: PROTO.into(), v: version, id: crate::random_hex(16), body }
    }

    pub fn to_bytes(&self) -> Vec<u8> {
        serde_json::to_vec(self).expect("FDP messages always serialize")
    }

    /// Parse and minimally validate an incoming datagram. The caller checks `v`.
    /// Returns `None` for oversized, malformed, foreign or unknown messages.
    pub fn parse(data: &[u8]) -> Option<Self> {
        if data.len() > MAX_PAYLOAD {
            return None;
        }
        let msg: Message = serde_json::from_slice(data).ok()?;
        (msg.proto == PROTO && msg.v >= 1).then_some(msg)
    }
}

/// Plaintext inside `set_ip.enc` (spec §4.3).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SetIpPayload {
    pub user: String,
    pub password: String,
    pub config: NetConfig,
}
