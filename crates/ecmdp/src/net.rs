//! Network configuration types, IPv4 helpers, validation (spec §4.3.1) and
//! the suggested configuration for the "Change IP" dialog (spec §8.4).

use serde::ser::SerializeMap;
use serde::{Deserialize, Serialize, Serializer};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum NetMode {
    Dhcp,
    Static,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct StaticAddress {
    /// CIDR, e.g. "192.168.1.100/24"
    pub address: String,
    #[serde(default)]
    pub gateway: Option<String>,
}

/// Current network state as reported in `announce` (spec §4.1).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct NetState {
    pub interface: String,
    pub mode: NetMode,
    #[serde(default)]
    pub address: Option<String>,
    #[serde(default)]
    pub gateway: Option<String>,
    #[serde(default)]
    pub dns: Vec<String>,
    #[serde(default)]
    pub fallback: Option<StaticAddress>,
}

/// Requested configuration in `set_ip` (spec §4.3).
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct NetConfig {
    pub mode: NetMode,
    #[serde(default)]
    pub address: Option<String>,
    #[serde(default)]
    pub gateway: Option<String>,
    #[serde(default)]
    pub dns: Vec<String>,
    #[serde(default)]
    pub fallback: Option<StaticAddress>,
}

/// Wire shape as in the spec: `static` carries `address`/`gateway`, `dhcp`
/// carries `fallback`.
impl Serialize for NetConfig {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        let mut map = s.serialize_map(None)?;
        map.serialize_entry("mode", &self.mode)?;
        match self.mode {
            NetMode::Static => {
                if let Some(address) = &self.address {
                    map.serialize_entry("address", address)?;
                }
                map.serialize_entry("gateway", &self.gateway)?;
            }
            NetMode::Dhcp => map.serialize_entry("fallback", &self.fallback)?,
        }
        map.serialize_entry("dns", &self.dns)?;
        map.end()
    }
}

// --- IPv4 helpers ---

/// Dotted quad to integer. Accepts exactly four decimal parts of 1–3 digits.
pub fn ip_to_int(ip: &str) -> Option<u32> {
    let parts: Vec<&str> = ip.split('.').collect();
    if parts.len() != 4 {
        return None;
    }
    let mut n: u32 = 0;
    for part in parts {
        if part.is_empty() || part.len() > 3 || !part.bytes().all(|b| b.is_ascii_digit()) {
            return None;
        }
        let octet: u32 = part.parse().ok()?;
        if octet > 255 {
            return None;
        }
        n = (n << 8) | octet;
    }
    Some(n)
}

pub fn int_to_ip(n: u32) -> String {
    std::net::Ipv4Addr::from(n).to_string()
}

pub fn prefix_mask(prefix: u8) -> u32 {
    if prefix == 0 { 0 } else { u32::MAX << (32 - u32::from(prefix)) }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Cidr {
    pub ip: String,
    pub prefix: u8,
    pub ip_int: u32,
    pub network: u32,
    pub broadcast: u32,
}

pub fn parse_cidr(cidr: &str) -> Option<Cidr> {
    let (ip, prefix) = cidr.trim().split_once('/')?;
    if prefix.is_empty() || prefix.len() > 2 || !prefix.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let prefix: u8 = prefix.parse().ok()?;
    let ip_int = ip_to_int(ip)?;
    if prefix > 32 {
        return None;
    }
    let mask = prefix_mask(prefix);
    let network = ip_int & mask;
    Some(Cidr { ip: ip.to_string(), prefix, ip_int, network, broadcast: network | !mask })
}

/// True if `ip` lies within the subnet given as CIDR.
pub fn in_subnet(ip: &str, cidr: &str) -> bool {
    match (parse_cidr(cidr), ip_to_int(ip)) {
        (Some(c), Some(n)) => n & prefix_mask(c.prefix) == c.network,
        _ => false,
    }
}

pub fn strip_prefix(cidr: &str) -> &str {
    cidr.split('/').next().unwrap_or(cidr)
}

fn is_unicast(n: u32) -> bool {
    let first = n >> 24;
    first != 0 && first != 127 && first < 224
}

// --- Configuration validation (spec §4.3.1) ---

fn validate_address(cidr: Option<&str>, label: &str) -> Result<(), String> {
    let cidr = cidr.filter(|c| !c.is_empty()).ok_or_else(|| format!("{label} is required"))?;
    let c = parse_cidr(cidr).ok_or_else(|| format!("{label} must be in the form a.b.c.d/prefix"))?;
    if !(1..=30).contains(&c.prefix) {
        return Err(format!("{label}: prefix must be between 1 and 30"));
    }
    if !is_unicast(c.ip_int) {
        return Err(format!("{label} must be a unicast address"));
    }
    if c.ip_int == c.network || c.ip_int == c.broadcast {
        return Err(format!("{label} must not be the network or broadcast address"));
    }
    Ok(())
}

fn validate_gateway(gateway: Option<&str>, cidr: &str, label: &str) -> Result<(), String> {
    let Some(gateway) = gateway.filter(|g| !g.is_empty()) else { return Ok(()) };
    match ip_to_int(gateway) {
        Some(g) if is_unicast(g) => {}
        _ => return Err(format!("{label} is not a valid address")),
    }
    if !in_subnet(gateway, cidr) {
        return Err(format!("{label} must be within {cidr}"));
    }
    if gateway == strip_prefix(cidr) {
        return Err(format!("{label} must differ from the device address"));
    }
    Ok(())
}

/// Returns an error message if the configuration is invalid.
pub fn validate_config(config: &NetConfig) -> Result<(), String> {
    match config.mode {
        NetMode::Static => {
            let address = config.address.as_deref();
            validate_address(address, "Address")?;
            validate_gateway(config.gateway.as_deref(), address.unwrap_or_default(), "Gateway")?;
        }
        NetMode::Dhcp => {
            if let Some(fb) = &config.fallback {
                validate_address(Some(&fb.address), "Fallback address")?;
                validate_gateway(fb.gateway.as_deref(), &fb.address, "Fallback gateway")?;
            }
        }
    }
    if config.dns.len() > 3 {
        return Err("At most 3 DNS servers are allowed".into());
    }
    for server in &config.dns {
        match ip_to_int(server) {
            Some(n) if is_unicast(n) => {}
            _ => return Err(format!("DNS server {server} is not a valid address")),
        }
    }
    Ok(())
}

// --- Suggested configuration for the "Change IP" dialog (spec §8.4) ---

fn is_link_local(ip: &str) -> bool {
    in_subnet(ip, "169.254.0.0/16")
}

/// Uniform random integer in `min..=max`.
fn random_int(min: u32, max: u32) -> u32 {
    let mut buf = [0u8; 4];
    getrandom::fill(&mut buf).expect("OS random number generator unavailable");
    min + u32::from_le_bytes(buf) % (max - min + 1)
}

/// Propose a configuration that makes the device reachable from the client
/// interface `local_cidr`. The user can edit it; we cannot check whether the
/// address is free.
pub fn suggest_config(local_cidr: &str, current: &NetState) -> NetConfig {
    let local = parse_cidr(local_cidr);
    let static_config = |address: String| NetConfig {
        mode: NetMode::Static,
        address: Some(address),
        gateway: None,
        dns: vec![],
        fallback: None,
    };

    // A static camera in a normal (non-APIPA) network: DHCP is the likely fix.
    if let Some(l) = &local
        && !is_link_local(&l.ip)
        && current.mode == NetMode::Static
    {
        return NetConfig {
            mode: NetMode::Dhcp,
            address: None,
            gateway: None,
            dns: vec![],
            fallback: current.fallback.clone(),
        };
    }

    let Some(local) = local.filter(|l| l.prefix <= 30) else {
        return static_config(String::new());
    };

    if is_link_local(&local.ip) {
        // RFC 3927: 169.254.1.0 – 169.254.254.255
        loop {
            let address = format!("169.254.{}.{}", random_int(1, 254), random_int(1, 254));
            if address != local.ip {
                return static_config(format!("{address}/16"));
            }
        }
    }

    // Pick an address near the top of the local subnet, avoiding the PC's own address.
    let hosts = local.broadcast - local.network - 1;
    loop {
        let candidate = local.broadcast - 1 - random_int(0, 19.min(hosts - 1));
        if candidate != local.ip_int || hosts <= 1 {
            return static_config(format!("{}/{}", int_to_ip(candidate), local.prefix));
        }
    }
}
