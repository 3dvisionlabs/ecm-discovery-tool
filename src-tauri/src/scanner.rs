//! Camera list: legacy mDNS discovery and ECMDP discovery, merging of both
//! (spec §9), TCP health checks and camera status (spec §8.3).

use std::collections::{HashMap, HashSet};
use std::net::Ipv4Addr;
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Duration;

use ecmdp::Body;
use tauri::async_runtime::spawn;
use tokio::net::TcpStream;
use tokio::sync::mpsc;
use tokio::time::{Instant, MissedTickBehavior, interval, interval_at, sleep, timeout};

use crate::client::{self, Discovered, LocalInterface};
use crate::mdns;
use crate::trust::TrustStore;
use crate::types::{Camera, CameraStatus, EcmdpInfo, now_ms};

/// How often to run a discovery scan
const DISCOVERY_INTERVAL: Duration = Duration::from_secs(30);
/// How often to check camera health via TCP connect
const HEALTH_CHECK_INTERVAL: Duration = Duration::from_secs(10);
/// TCP connect timeout per camera
const TCP_TIMEOUT: Duration = Duration::from_secs(3);
/// An ECMDP camera that has not answered for this long counts as offline
const ECMDP_STALE_AFTER: Duration = Duration::from_secs(2 * 30 + 5);
/// Minimum time between two refreshes
const REFRESH_THROTTLE: Duration = Duration::from_secs(2);

pub enum Event {
    Found(Camera),
    Updated(Camera),
    /// Legacy mDNS entry merged into an ECMDP entry
    Removed(String),
}

pub type Emit = Arc<dyn Fn(Event) + Send + Sync>;

struct EcmdpMeta {
    local: LocalInterface,
    last_announce: Instant,
}

struct Entry {
    camera: Camera,
    /// Changes whenever the entry is replaced, so late probe results for an
    /// older entry are ignored
    generation: u64,
    meta: Option<EcmdpMeta>,
}

#[derive(Default)]
struct State {
    cameras: HashMap<String, Entry>,
    /// Cameras dismissed by refresh — only re-shown if they pass a TCP check
    dismissed: HashSet<String>,
    next_generation: u64,
    refresh_blocked_until: Option<Instant>,
    ecmdp_running: bool,
}

impl State {
    fn insert(&mut self, camera: Camera, meta: Option<EcmdpMeta>) {
        self.next_generation += 1;
        let entry = Entry { camera, generation: self.next_generation, meta };
        self.cameras.insert(entry.camera.id.clone(), entry);
    }
}

struct Inner {
    state: Mutex<State>,
    trust: Arc<Mutex<TrustStore>>,
    emit: Emit,
}

#[derive(Clone)]
pub struct Scanner {
    inner: Arc<Inner>,
}

fn status_for(camera: &Camera, meta: Option<&EcmdpMeta>, reachable: bool) -> CameraStatus {
    if reachable {
        return CameraStatus::Online;
    }
    if camera.ecmdp.is_none() {
        return CameraStatus::Offline;
    }
    match meta {
        Some(meta) if meta.last_announce.elapsed() <= ECMDP_STALE_AFTER => {
            if !camera.ip.is_empty() && ecmdp::in_subnet(&camera.ip, &meta.local.cidr) {
                CameraStatus::Unreachable
            } else {
                CameraStatus::OtherSubnet
            }
        }
        _ => CameraStatus::Offline,
    }
}

async fn probe(ip: &str, port: u16) -> bool {
    let Ok(ip) = ip.parse::<Ipv4Addr>() else { return false };
    matches!(timeout(TCP_TIMEOUT, TcpStream::connect((ip, port))).await, Ok(Ok(_)))
}

impl Scanner {
    pub fn new(trust: Arc<Mutex<TrustStore>>, emit: Emit) -> Self {
        Scanner { inner: Arc::new(Inner { state: Mutex::default(), trust, emit }) }
    }

    fn state(&self) -> MutexGuard<'_, State> {
        self.inner.state.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn emit_all(&self, events: Vec<Event>) {
        for event in events {
            (self.inner.emit)(event);
        }
    }

    pub fn start(&self) {
        let s = self.clone();
        spawn(async move {
            let mut timer = interval(DISCOVERY_INTERVAL);
            timer.set_missed_tick_behavior(MissedTickBehavior::Delay);
            loop {
                timer.tick().await;
                s.run_discovery();
            }
        });
        let s = self.clone();
        spawn(async move {
            let mut timer = interval_at(Instant::now() + HEALTH_CHECK_INTERVAL, HEALTH_CHECK_INTERVAL);
            timer.set_missed_tick_behavior(MissedTickBehavior::Delay);
            loop {
                timer.tick().await;
                s.check_all_cameras();
            }
        });
    }

    pub fn refresh(&self) {
        {
            let mut st = self.state();
            let now = Instant::now();
            if st.refresh_blocked_until.is_some_and(|until| now < until) {
                return;
            }
            st.refresh_blocked_until = Some(now + REFRESH_THROTTLE);

            // Dismiss offline cameras — they won't reappear unless the TCP check passes
            let offline: Vec<String> =
                st.cameras.values().filter(|e| !e.camera.online).map(|e| e.camera.id.clone()).collect();
            for id in offline {
                st.cameras.remove(&id);
                st.dismissed.insert(id);
            }
        }
        self.run_discovery();
        self.check_all_cameras();
    }

    pub fn get_all(&self) -> Vec<Camera> {
        self.state().cameras.values().map(|e| e.camera.clone()).collect()
    }

    pub fn get_by_id(&self, id: &str) -> Option<Camera> {
        self.state().cameras.get(id).map(|e| e.camera.clone())
    }

    /// ECMDP data plus the PC interface the camera answered on — for targeted requests.
    pub fn get_ecmdp(&self, id: &str) -> Option<(EcmdpInfo, LocalInterface)> {
        let st = self.state();
        let entry = st.cameras.get(id)?;
        Some((entry.camera.ecmdp.clone()?, entry.meta.as_ref()?.local.clone()))
    }

    /// Run a discovery cycle after `delay`, e.g. after an IP change.
    pub fn rediscover(&self, delay: Duration) {
        let s = self.clone();
        spawn(async move {
            sleep(delay).await;
            s.run_discovery();
        });
    }

    /// Re-evaluate the key trust state after the user trusted a key.
    pub fn update_key_trust(&self, id: &str) {
        let camera = {
            let mut st = self.state();
            let Some(entry) = st.cameras.get_mut(id) else { return };
            let Some(info) = entry.camera.ecmdp.as_mut() else { return };
            info.key_trust =
                self.inner.trust.lock().unwrap_or_else(|e| e.into_inner()).check(&info.serial, &info.key_fp);
            entry.camera.clone()
        };
        self.emit_all(vec![Event::Updated(camera)]);
    }

    fn run_discovery(&self) {
        self.discover_ecmdp();

        let (tx, mut rx) = mpsc::unbounded_channel();
        spawn(mdns::browse(tx));
        let s = self.clone();
        spawn(async move {
            while let Some((hostname, ip)) = rx.recv().await {
                match ip {
                    Some(ip) => s.add_camera(&hostname, ip),
                    None => {
                        let s = s.clone();
                        spawn(async move {
                            if let Some(ip) = mdns::resolve(&hostname).await {
                                s.add_camera(&hostname, ip);
                            }
                        });
                    }
                }
            }
        });
    }

    // --- Discovery via ECMDP (discovery-protocol.md) ---

    fn discover_ecmdp(&self) {
        {
            let mut st = self.state();
            if st.ecmdp_running {
                return;
            }
            st.ecmdp_running = true;
        }
        let s = self.clone();
        spawn(async move {
            for result in client::discover().await {
                s.add_ecmdp_camera(result);
            }
            s.state().ecmdp_running = false;
        });
    }

    fn add_ecmdp_camera(&self, discovered: Discovered) {
        let Body::Announce { device, net, caps, set_ip, versions } = discovered.message.body else { return };
        let id = format!("sn-{}", device.serial);
        let ip = net.address.as_deref().map(ecmdp::strip_prefix).unwrap_or_default().to_string();
        let key_trust =
            self.inner.trust.lock().unwrap_or_else(|e| e.into_inner()).check(&device.serial, &device.key_fp);
        let mut events = Vec::new();

        {
            let mut st = self.state();
            st.dismissed.remove(&id);

            // A legacy mDNS entry with the same IP is the same camera (spec §9)
            let legacy: Vec<String> = st
                .cameras
                .values()
                .filter(|e| e.camera.ecmdp.is_none() && !ip.is_empty() && e.camera.ip == ip)
                .map(|e| e.camera.id.clone())
                .collect();
            for other in legacy {
                st.cameras.remove(&other);
                events.push(Event::Removed(other));
            }

            let info = EcmdpInfo {
                serial: device.serial,
                mac: device.mac,
                vendor: device.vendor,
                model: device.model,
                fw_version: device.fw_version,
                net,
                caps,
                set_ip,
                key_fp: device.key_fp,
                key_trust,
                version: ecmdp::negotiate_version(versions.as_deref()),
                versions: versions.unwrap_or_else(|| vec![1]),
                local_address: discovered.local.address.to_string(),
                local_cidr: discovered.local.cidr.clone(),
            };

            let existing = st.cameras.get(&id).map(|e| (e.camera.last_seen, e.camera.online, e.camera.ip.clone()));
            let meta = EcmdpMeta { local: discovered.local, last_announce: Instant::now() };
            let mut camera = Camera {
                id: id.clone(),
                hostname: device.hostname,
                ip: ip.clone(),
                port: device.web.port,
                scheme: device.web.scheme,
                last_seen: existing.as_ref().map_or_else(now_ms, |e| e.0),
                online: existing.as_ref().is_some_and(|e| e.1),
                status: CameraStatus::Offline,
                ecmdp: Some(info),
            };
            // Keep "online" until the TCP check says otherwise, to avoid flicker
            let was_online_here = existing.as_ref().is_some_and(|e| e.1 && e.2 == ip);
            camera.status =
                if was_online_here { CameraStatus::Online } else { status_for(&camera, Some(&meta), false) };

            events.push(if existing.is_some() { Event::Updated(camera.clone()) } else { Event::Found(camera.clone()) });
            st.insert(camera, Some(meta));
        }

        self.emit_all(events);
        self.check_camera(&id);
    }

    // --- Legacy cameras (mDNS) ---

    fn add_camera(&self, hostname: &str, ip: Ipv4Addr) {
        let ip = ip.to_string();
        let id = format!("{hostname}-{ip}");
        let new_camera = || Camera {
            id: id.clone(),
            hostname: hostname.to_string(),
            ip: ip.clone(),
            port: 443,
            scheme: "https".into(),
            last_seen: now_ms(),
            online: false,
            status: CameraStatus::Offline,
            ecmdp: None,
        };

        let mut st = self.state();
        // Already known via ECMDP — that entry wins (spec §9)
        if st.cameras.values().any(|e| e.camera.ecmdp.is_some() && e.camera.ip == ip) {
            return;
        }
        if st.cameras.contains_key(&id) {
            // Already known — just verify it's still reachable
            drop(st);
            return self.check_camera(&id);
        }
        if st.dismissed.contains(&id) {
            // Dismissed by refresh — silently check if it's back online
            drop(st);
            return self.check_camera_dismissed(new_camera());
        }

        // New camera — notify the frontend and verify status via TCP
        let camera = new_camera();
        st.insert(camera.clone(), None);
        drop(st);
        self.emit_all(vec![Event::Found(camera)]);
        self.check_camera(&id);
    }

    /// Check a dismissed camera — only re-add it if it's actually reachable.
    fn check_camera_dismissed(&self, mut camera: Camera) {
        let s = self.clone();
        spawn(async move {
            if !probe(&camera.ip, camera.port).await {
                return;
            }
            camera.online = true;
            camera.status = CameraStatus::Online;
            camera.last_seen = now_ms();
            {
                let mut st = s.state();
                st.dismissed.remove(&camera.id);
                st.insert(camera.clone(), None);
            }
            s.emit_all(vec![Event::Found(camera)]);
        });
    }

    // --- TCP health checks ---

    fn check_all_cameras(&self) {
        let ids: Vec<String> = self.state().cameras.keys().cloned().collect();
        for id in ids {
            self.check_camera(&id);
        }
    }

    fn check_camera(&self, id: &str) {
        let Some((ip, port, generation)) =
            self.state().cameras.get(id).map(|e| (e.camera.ip.clone(), e.camera.port, e.generation))
        else {
            return;
        };
        let s = self.clone();
        let id = id.to_string();
        spawn(async move {
            let reachable = probe(&ip, port).await;
            let updated = {
                let mut st = s.state();
                // Ignore results for entries that were replaced or removed meanwhile
                let Some(entry) = st.cameras.get_mut(&id).filter(|e| e.generation == generation) else { return };
                if reachable {
                    entry.camera.last_seen = now_ms();
                }
                let status = status_for(&entry.camera, entry.meta.as_ref(), reachable);
                if status == entry.camera.status && reachable == entry.camera.online {
                    return;
                }
                entry.camera.status = status;
                entry.camera.online = reachable;
                entry.camera.clone()
            };
            s.emit_all(vec![Event::Updated(updated)]);
        });
    }
}
