//! ECMDP client (spec §8): broadcast discovery and targeted requests.

use std::collections::{HashMap, HashSet};
use std::net::{Ipv4Addr, SocketAddr};
use std::sync::OnceLock;
use std::time::Duration;

use ecmdp::{Body, Message};
use tokio::net::UdpSocket;
use tokio::time::{Instant, timeout_at};

// Discovery: send at t = 0, 1, 2 s and collect until t = 3 s (spec §8.2)
const DISCOVER_SENDS: u32 = 3;
const DISCOVER_SPACING: Duration = Duration::from_secs(1);
const DISCOVER_WINDOW: Duration = Duration::from_secs(3);
// Targeted requests: the device must answer within 2.5 s (spec §4.3)
const REQUEST_TIMEOUT: Duration = Duration::from_millis(2500);

pub fn port() -> u16 {
    static PORT: OnceLock<u16> = OnceLock::new();
    *PORT.get_or_init(|| {
        std::env::var("ECMDP_PORT").ok().and_then(|p| p.parse().ok()).filter(|&p| p != 0).unwrap_or(ecmdp::DEFAULT_PORT)
    })
}

/// Also scan via 127.0.0.1 — for testing against the local mock without a network.
fn include_loopback() -> bool {
    std::env::var("ECMDP_LOOPBACK").is_ok_and(|v| v == "1")
}

#[derive(Debug, Clone)]
pub struct LocalInterface {
    pub name: String,
    pub address: Ipv4Addr,
    pub cidr: String,
    pub broadcast: Ipv4Addr,
}

#[derive(Debug, Clone)]
pub struct Discovered {
    pub message: Message,
    /// Client interface on which the device answered
    pub local: LocalInterface,
}

impl Discovered {
    fn announced_address(&self) -> Option<&str> {
        match &self.message.body {
            Body::Announce { net, .. } => net.address.as_deref().map(ecmdp::strip_prefix),
            _ => None,
        }
    }
}

// Container, VM and VPN interfaces: still scanned, but a camera that answers on
// several interfaces is assigned to a physical one if possible.
const VIRTUAL_PREFIXES: &[&str] = &[
    "docker",
    "br-",
    "veth",
    "virbr",
    "vmnet",
    "vboxnet",
    "vethernet",
    "tun",
    "tap",
    "wg",
    "zt",
    "tailscale",
    "utun",
    "lo",
];

fn is_virtual(local: &LocalInterface) -> bool {
    let name = local.name.to_lowercase();
    VIRTUAL_PREFIXES.iter().any(|p| name.starts_with(p)) || local.broadcast == local.address
}

pub fn local_interfaces() -> Vec<LocalInterface> {
    let loopback = include_loopback();
    let Ok(ifaces) = if_addrs::get_if_addrs() else { return vec![] };
    ifaces
        .into_iter()
        .filter_map(|iface| {
            let if_addrs::IfAddr::V4(v4) = &iface.addr else { return None };
            let internal = iface.is_loopback();
            if internal && !loopback {
                return None;
            }
            let prefix = u32::from(v4.netmask).count_ones();
            let cidr = format!("{}/{prefix}", v4.ip);
            let c = ecmdp::parse_cidr(&cidr)?;
            let broadcast = if internal { v4.ip } else { Ipv4Addr::from(c.broadcast) };
            Some(LocalInterface { name: iface.name.clone(), address: v4.ip, cidr, broadcast })
        })
        .collect()
}

/// Send one datagram to the interface's directed broadcast and to 255.255.255.255 (spec §8.1).
async fn send_broadcast(sock: &UdpSocket, local: &LocalInterface, msg: &Message) {
    let data = msg.to_bytes();
    let destinations = if local.broadcast == local.address {
        vec![local.address] // loopback: plain unicast to the mock
    } else {
        vec![local.broadcast, Ipv4Addr::BROADCAST]
    };
    for dest in destinations {
        if let Err(err) = sock.send_to(&data, (dest, port())).await {
            eprintln!("ECMDP send to {dest} via {} failed: {err}", local.name);
        }
    }
}

async fn open_socket(local: &LocalInterface) -> std::io::Result<UdpSocket> {
    let sock = UdpSocket::bind(SocketAddr::from((local.address, 0))).await?;
    sock.set_broadcast(true)?;
    Ok(sock)
}

/// Receive one ECMDP message until `deadline`. `None` on timeout; socket
/// errors (e.g. ICMP "port unreachable" on Windows) are skipped.
async fn recv_until(sock: &UdpSocket, deadline: Instant) -> Option<Message> {
    let mut buf = [0u8; 2048];
    loop {
        match timeout_at(deadline, sock.recv_from(&mut buf)).await {
            Err(_) => return None,
            Ok(Err(_)) => continue,
            Ok(Ok((n, _))) => {
                if let Some(msg) = Message::parse(&buf[..n]) {
                    return Some(msg);
                }
            }
        }
    }
}

async fn discover_on(local: LocalInterface) -> Vec<Discovered> {
    let sock = match open_socket(&local).await {
        Ok(sock) => sock,
        Err(err) => {
            eprintln!("ECMDP: cannot open socket on {} ({}): {err}", local.name, local.address);
            return vec![];
        }
    };

    let start = Instant::now();
    let end = start + DISCOVER_WINDOW;
    let mut ids = HashSet::new();
    let mut found = Vec::new();
    let mut sent = 0;
    loop {
        let now = Instant::now();
        let next_send = start + DISCOVER_SPACING * sent;
        if sent < DISCOVER_SENDS && now >= next_send {
            let msg = Message::new(ecmdp::DISCOVERY_VERSION, Body::Discover {});
            ids.insert(msg.id.clone());
            send_broadcast(&sock, &local, &msg).await;
            sent += 1;
            continue;
        }
        if now >= end {
            break;
        }
        let wake = if sent < DISCOVER_SENDS { next_send.min(end) } else { end };
        if let Some(msg) = recv_until(&sock, wake).await
            && matches!(msg.body, Body::Announce { .. })
            && ids.contains(&msg.id)
        {
            found.push(Discovered { message: msg, local: local.clone() });
        }
    }
    found
}

/// Run one discovery cycle on all interfaces. Returns after the collection
/// window with one entry per device serial.
pub async fn discover() -> Vec<Discovered> {
    let tasks: Vec<_> = local_interfaces().into_iter().map(|local| tokio::spawn(discover_on(local))).collect();

    let mut by_serial: HashMap<String, Vec<Discovered>> = HashMap::new();
    for task in tasks {
        for d in task.await.unwrap_or_default() {
            if let Body::Announce { device, .. } = &d.message.body {
                by_serial.entry(device.serial.clone()).or_default().push(d);
            }
        }
    }

    // A device may answer on several interfaces. Prefer an interface whose
    // subnet contains the device, then physical over virtual interfaces.
    let score = |d: &Discovered| {
        let reachable = d.announced_address().is_some_and(|ip| ecmdp::in_subnet(ip, &d.local.cidr));
        (if reachable { 0 } else { 2 }) + u8::from(is_virtual(&d.local))
    };
    by_serial
        .into_values()
        .filter_map(|candidates| {
            let best = candidates.iter().map(score).min()?;
            // Newest announce among the best candidates
            candidates.into_iter().rev().find(|c| score(c) == best)
        })
        .collect()
}

/// Send a targeted request via the given interface and wait for the response
/// with the same id. `None` on timeout. A `version_error` from the device is
/// always accepted as a response.
pub async fn request(local: &LocalInterface, msg: &Message, expected: &[&str]) -> std::io::Result<Option<Message>> {
    let sock = open_socket(local).await?;
    let deadline = Instant::now() + REQUEST_TIMEOUT;
    send_broadcast(&sock, local, msg).await;
    while let Some(res) = recv_until(&sock, deadline).await {
        let kind = res.body.type_name();
        if res.id == msg.id && (expected.contains(&kind) || kind == "version_error") {
            return Ok(Some(res));
        }
    }
    Ok(None)
}
