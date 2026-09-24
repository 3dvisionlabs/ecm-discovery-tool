//! Legacy discovery via mDNS (cameras without ECMDP): `_ssh._tcp` services
//! whose instance name starts with `ecm-`.
//!
//! macOS: `dns-sd`, Linux: `avahi-browse`, Windows: raw mDNS query from an
//! ephemeral port. All variants are compiled everywhere; `browse` picks one.

use std::net::{Ipv4Addr, SocketAddr};
use std::process::Stdio;
use std::time::Duration;

use simple_dns::rdata::RData;
use simple_dns::{CLASS, Name, Packet, QCLASS, QTYPE, Question, TYPE};
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::net::UdpSocket;
use tokio::process::Command;
use tokio::sync::mpsc::UnboundedSender;
use tokio::time::{Instant, timeout, timeout_at};

const MDNS_MULTICAST: Ipv4Addr = Ipv4Addr::new(224, 0, 0, 251);
const MDNS_PORT: u16 = 5353;
/// How long to collect mDNS browse results
const BROWSE_DURATION: Duration = Duration::from_secs(5);

/// A camera found via mDNS: hostname (without `.local`) and, if the tool
/// resolved it, its IPv4 address.
pub type Found = (String, Option<Ipv4Addr>);

fn is_camera(name: &str) -> bool {
    name.to_lowercase().starts_with("ecm-")
}

fn strip_local(name: &str) -> &str {
    let name = name.strip_suffix('.').unwrap_or(name);
    name.strip_suffix(".local").unwrap_or(name)
}

pub async fn browse(tx: UnboundedSender<Found>) {
    if cfg!(target_os = "macos") {
        browse_dns_sd(tx).await;
    } else if cfg!(windows) {
        browse_packet(tx).await;
    } else {
        browse_avahi(tx).await;
    }
}

/// macOS: `dns-sd -B` streams continuously, so stop it after BROWSE_DURATION.
/// The instance name for SSH services equals the hostname.
async fn browse_dns_sd(tx: UnboundedSender<Found>) {
    let child = Command::new("dns-sd")
        .args(["-B", "_ssh._tcp", "local."])
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .kill_on_drop(true)
        .spawn();
    let mut child = match child {
        Ok(child) => child,
        Err(err) => return eprintln!("dns-sd error: {err}"),
    };
    let Some(stdout) = child.stdout.take() else { return };
    let mut lines = BufReader::new(stdout).lines();
    let mut seen = std::collections::HashSet::new();
    let deadline = Instant::now() + BROWSE_DURATION;
    while let Ok(Ok(Some(line))) = timeout_at(deadline, lines.next_line()).await {
        if let Some(hostname) = parse_dns_sd_line(&line)
            && seen.insert(hostname.clone())
        {
            let _ = tx.send((hostname, None));
        }
    }
    let _ = child.kill().await;
}

/// Lines look like: "15:20:25.880  Add  3  25 local.  _ssh._tcp.  ecm-232250000988"
fn parse_dns_sd_line(line: &str) -> Option<String> {
    let pos = line.find("_ssh._tcp.")?;
    if !line[..pos].split_whitespace().any(|w| w == "Add") {
        return None;
    }
    let hostname = line[pos + "_ssh._tcp.".len()..].trim();
    is_camera(hostname).then(|| hostname.to_string())
}

/// Linux: `avahi-browse` one-shot discovery with resolved addresses.
async fn browse_avahi(tx: UnboundedSender<Found>) {
    let child = Command::new("avahi-browse")
        .args(["-r", "-p", "-t", "_ssh._tcp"])
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .kill_on_drop(true)
        .spawn();
    let mut child = match child {
        Ok(child) => child,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
            return eprintln!("avahi-browse not found. Install avahi-utils (e.g., sudo apt install avahi-utils).");
        }
        Err(err) => return eprintln!("avahi-browse error: {err}"),
    };
    let Some(stdout) = child.stdout.take() else { return };
    let mut lines = BufReader::new(stdout).lines();
    // -t ends after the cache dump; the limit only guards against a hanging daemon
    let deadline = Instant::now() + 3 * BROWSE_DURATION;
    while let Ok(Ok(Some(line))) = timeout_at(deadline, lines.next_line()).await {
        if let Some(found) = parse_avahi_line(&line) {
            let _ = tx.send(found);
        }
    }
    let _ = child.kill().await;
}

/// Parseable format: =;iface;protocol;name;type;domain;hostname;address;port;txt
fn parse_avahi_line(line: &str) -> Option<Found> {
    let fields: Vec<&str> = line.split(';').collect();
    if fields.first() != Some(&"=") || !is_camera(fields.get(3)?) {
        return None;
    }
    let hostname = strip_local(fields.get(6)?);
    if hostname.is_empty() {
        return None;
    }
    // Use the resolved address if it's IPv4, otherwise the caller resolves the name
    let ip = fields.get(7).and_then(|a| a.parse::<Ipv4Addr>().ok());
    Some((hostname.to_string(), ip))
}

/// Windows: send a raw mDNS PTR query for _ssh._tcp.local via UDP multicast.
/// Uses an ephemeral source port (not 5353), so responses come back as unicast
/// per RFC 6762 §5.5. This avoids binding to port 5353 and conflicts with
/// other mDNS tools (e.g. Python zeroconf) running on the same machine.
async fn browse_packet(tx: UnboundedSender<Found>) {
    let mut query = Packet::new_query(0);
    query.questions.push(Question::new(
        Name::new_unchecked("_ssh._tcp.local"),
        QTYPE::TYPE(TYPE::PTR),
        QCLASS::CLASS(CLASS::IN),
        false,
    ));
    let Ok(data) = query.build_bytes_vec() else { return };

    let sock = match UdpSocket::bind(SocketAddr::from((Ipv4Addr::UNSPECIFIED, 0))).await {
        Ok(sock) => sock,
        Err(err) => return eprintln!("mDNS UDP error: {err}"),
    };
    if let Err(err) = sock.send_to(&data, (MDNS_MULTICAST, MDNS_PORT)).await {
        return eprintln!("mDNS send error: {err}");
    }

    let deadline = Instant::now() + BROWSE_DURATION;
    let mut buf = [0u8; 9000];
    while let Ok(result) = timeout_at(deadline, sock.recv_from(&mut buf)).await {
        let Ok((n, _)) = result else { continue };
        for found in parse_mdns_response(&buf[..n]) {
            let _ = tx.send(found);
        }
    }
}

/// Extract PTR → hostname, A → IP address. A single response may contain
/// several answers and additional records.
fn parse_mdns_response(data: &[u8]) -> Vec<Found> {
    let Ok(packet) = Packet::parse(data) else { return vec![] };
    let records = || packet.answers.iter().chain(packet.additional_records.iter());

    let a_records: std::collections::HashMap<String, Ipv4Addr> = records()
        .filter_map(|rec| match &rec.rdata {
            RData::A(a) => Some((strip_local(&rec.name.to_string()).to_lowercase(), Ipv4Addr::from(a.address))),
            _ => None,
        })
        .collect();

    records()
        .filter_map(|rec| {
            let RData::PTR(ptr) = &rec.rdata else { return None };
            // PTR data looks like "ecm-12345678._ssh._tcp.local"
            let target = ptr.0.to_string();
            let target = target.strip_suffix('.').unwrap_or(&target);
            let instance = target.strip_suffix("._ssh._tcp.local")?;
            is_camera(instance).then(|| (instance.to_string(), a_records.get(&instance.to_lowercase()).copied()))
        })
        .collect()
}

/// Resolve `<hostname>.local` via the OS resolver (IPv4 only).
pub async fn resolve(hostname: &str) -> Option<Ipv4Addr> {
    let addrs = timeout(BROWSE_DURATION, tokio::net::lookup_host(format!("{hostname}.local:0"))).await.ok()?.ok()?;
    addrs
        .filter_map(|a| match a {
            SocketAddr::V4(v4) => Some(*v4.ip()),
            SocketAddr::V6(_) => None,
        })
        .next()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dns_sd_line() {
        let line = "15:20:25.880  Add        3  25 local.               _ssh._tcp.           ecm-232250000988";
        assert_eq!(parse_dns_sd_line(line).as_deref(), Some("ecm-232250000988"));
        assert_eq!(parse_dns_sd_line(&line.replace("Add", "Rmv")), None);
        assert_eq!(parse_dns_sd_line(&line.replace("ecm-", "pc-")), None);
    }

    #[test]
    fn avahi_line() {
        let line = "=;eth0;IPv4;ecm-232250000988;SSH Remote Terminal;local;ecm-232250000988.local;192.168.1.100;22;";
        assert_eq!(parse_avahi_line(line), Some(("ecm-232250000988".into(), Some(Ipv4Addr::new(192, 168, 1, 100)))));
        assert_eq!(parse_avahi_line(&line.replace("192.168.1.100", "fe80::1")).unwrap().1, None);
        assert_eq!(parse_avahi_line(&line.replacen('=', "+", 1)), None);
    }

    #[test]
    fn mdns_response() {
        use simple_dns::ResourceRecord;
        use simple_dns::rdata::{A, PTR};
        let mut packet = Packet::new_reply(0);
        packet.answers.push(ResourceRecord::new(
            Name::new_unchecked("_ssh._tcp.local"),
            CLASS::IN,
            120,
            RData::PTR(PTR(Name::new_unchecked("ecm-12345678._ssh._tcp.local"))),
        ));
        packet.additional_records.push(ResourceRecord::new(
            Name::new_unchecked("ecm-12345678.local"),
            CLASS::IN,
            120,
            RData::A(A { address: u32::from(Ipv4Addr::new(10, 0, 0, 7)) }),
        ));
        let data = packet.build_bytes_vec().unwrap();
        assert_eq!(parse_mdns_response(&data), vec![("ecm-12345678".into(), Some(Ipv4Addr::new(10, 0, 0, 7)))]);
    }
}
