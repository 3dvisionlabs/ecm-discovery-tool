//! ECM Discovery Protocol (ECMDP) — see `discovery-protocol.md`.
//!
//! Messages, crypto, IPv4 helpers and configuration validation. Free of any
//! I/O so the client (Edge Camera Discovery) and the firmware
//! (`ecm-discovery-service`) can share it.

pub mod crypto;
pub mod message;
pub mod net;

pub use message::*;
pub use net::*;

/// Placeholder until the port is assigned (spec §10). Override with `ECMDP_PORT`.
pub const DEFAULT_PORT: u16 = 27270;
pub const PROTO: &str = "ecmdp";
pub const MAX_PAYLOAD: usize = 1400;

/// `discover`/`announce` are frozen at version 1 so any client can find any
/// device (spec §3.2). `announce.versions` lists the versions the device
/// speaks for targeted requests; the client uses the highest common one.
pub const DISCOVERY_VERSION: u32 = 1;
pub const CLIENT_VERSIONS: &[u32] = &[1];
pub const ENC_ALG: &str = "x25519-hkdf-sha256-aes256gcm";

/// Highest version both sides speak, or `None`. Devices without `versions` speak only v1.
pub fn negotiate_version(device_versions: Option<&[u32]>) -> Option<u32> {
    let device = device_versions.unwrap_or(&[1]);
    CLIENT_VERSIONS.iter().copied().filter(|v| device.contains(v)).max()
}

pub fn hkdf_info(version: u32) -> String {
    format!("ecmdp/{version} set_ip")
}

/// AAD for the `set_ip` encryption (spec §5.2).
pub fn set_ip_aad(version: u32, target: &Target, id: &str, challenge: &str, epk_hex: &str) -> String {
    [&format!("ecmdp/{version}"), "set_ip", &target.serial, &target.mac, id, challenge, epk_hex].join("\n")
}

/// Random bytes as lowercase hex, e.g. for message ids and challenges.
pub fn random_hex(bytes: usize) -> String {
    let mut buf = vec![0u8; bytes];
    getrandom::fill(&mut buf).expect("OS random number generator unavailable");
    hex::encode(buf)
}
