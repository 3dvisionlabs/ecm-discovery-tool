//! Camera list: legacy mDNS discovery and FDP discovery, merging of both
//! (spec §9), TCP health checks and camera status (spec §8.3).

use std::collections::{HashMap, HashSet};
use std::future::Future;
use std::net::Ipv4Addr;
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Duration;

use fdp::Body;
use tokio::net::TcpStream;
use tokio::runtime::Handle;
use tokio::sync::{mpsc, watch};
use tokio::time::{Instant, MissedTickBehavior, interval, interval_at, sleep, timeout};

use crate::client::{self, Discovered, LocalInterface};
use crate::mdns;
use crate::trust::TrustStore;
use crate::types::{Camera, CameraStatus, FdpInfo, now_ms};

/// How often to run a discovery scan
const DISCOVERY_INTERVAL: Duration = Duration::from_secs(30);
/// How often to check camera health via TCP connect
const HEALTH_CHECK_INTERVAL: Duration = Duration::from_secs(10);
/// TCP connect timeout per camera
const TCP_TIMEOUT: Duration = Duration::from_secs(3);
/// A camera that has not answered FDP or mDNS for this long counts as offline
const STALE_AFTER: Duration = Duration::from_secs(2 * 30 + 5);
/// Minimum time between two refreshes
const REFRESH_THROTTLE: Duration = Duration::from_secs(2);

pub enum Event {
    Found(Camera),
    Updated(Camera),
    /// Legacy mDNS entry merged into an FDP entry
    Removed(String),
}

pub type Emit = Arc<dyn Fn(Event) + Send + Sync>;

struct FdpMeta {
    local: LocalInterface,
    last_announce: Instant,
}

struct Entry {
    camera: Camera,
    /// Changes whenever the entry is replaced, so late probe results for an
    /// older entry are ignored
    generation: u64,
    meta: Option<FdpMeta>,
    /// Legacy cameras: when mDNS last reported them
    last_mdns: Option<Instant>,
}

#[derive(Default)]
struct State {
    cameras: HashMap<String, Entry>,
    /// Cameras dismissed by refresh — only re-shown if they pass a TCP check
    dismissed: HashSet<String>,
    next_generation: u64,
    refresh_blocked_until: Option<Instant>,
    fdp_running: bool,
}

impl State {
    fn insert(&mut self, camera: Camera, meta: Option<FdpMeta>, last_mdns: Option<Instant>) {
        self.next_generation += 1;
        let entry = Entry { camera, generation: self.next_generation, meta, last_mdns };
        self.cameras.insert(entry.camera.id.clone(), entry);
    }
}

struct Inner {
    state: Mutex<State>,
    trust: Arc<Mutex<TrustStore>>,
    emit: Emit,
    rt: Handle,
    /// Number of running scan tasks (discovery, resolving, TCP checks)
    pending: watch::Sender<usize>,
}

/// Counts a scan task as finished when dropped, also if it panics.
struct TaskDone(Scanner);

impl Drop for TaskDone {
    fn drop(&mut self) {
        self.0.inner.pending.send_modify(|n| *n -= 1);
    }
}

#[derive(Clone)]
pub struct Scanner {
    inner: Arc<Inner>,
}

/// Status from the web UI check (`reachable`). If it fails, a camera that
/// still answers FDP or mDNS is running, only its web UI does not respond.
fn status_for(camera: &Camera, meta: Option<&FdpMeta>, last_mdns: Option<Instant>, reachable: bool) -> CameraStatus {
    if reachable {
        return CameraStatus::Online;
    }
    if camera.fdp.is_none() {
        return match last_mdns {
            Some(seen) if seen.elapsed() <= STALE_AFTER => CameraStatus::Unreachable,
            _ => CameraStatus::Offline,
        };
    }
    match meta {
        Some(meta) if meta.last_announce.elapsed() <= STALE_AFTER => {
            if !camera.ip.is_empty() && fdp::in_subnet(&camera.ip, &meta.local.cidr) {
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
    /// `rt`: the runtime the background tasks run on.
    pub fn new(trust: Arc<Mutex<TrustStore>>, emit: Emit, rt: Handle) -> Self {
        let (pending, _) = watch::channel(0);
        Scanner { inner: Arc::new(Inner { state: Mutex::default(), trust, emit, rt, pending }) }
    }

    fn spawn<F: Future + Send + 'static>(&self, task: F) -> tokio::task::JoinHandle<F::Output>
    where
        F::Output: Send + 'static,
    {
        self.inner.rt.spawn(task)
    }

    /// Spawn a task that belongs to a scan; `settled` waits for it.
    fn track(&self, task: impl Future<Output = ()> + Send + 'static) {
        self.inner.pending.send_modify(|n| *n += 1);
        let done = TaskDone(self.clone());
        self.spawn(async move {
            let _done = done;
            task.await;
        });
    }

    /// Wait until all scan tasks have finished, e.g. after `refresh`: then the
    /// camera list reflects one complete scan.
    pub async fn settled(&self) {
        let mut rx = self.inner.pending.subscribe();
        let _ = rx.wait_for(|&n| n == 0).await;
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
        self.spawn(async move {
            let mut timer = interval(DISCOVERY_INTERVAL);
            timer.set_missed_tick_behavior(MissedTickBehavior::Delay);
            loop {
                timer.tick().await;
                s.run_discovery();
            }
        });
        let s = self.clone();
        self.spawn(async move {
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

            // Dismiss offline cameras — they won't reappear unless the TCP check passes.
            // Cameras that still answer (other subnet, web UI down) stay.
            let offline: Vec<String> = st
                .cameras
                .values()
                .filter(|e| e.camera.status == CameraStatus::Offline)
                .map(|e| e.camera.id.clone())
                .collect();
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

    /// FDP data plus the PC interface the camera answered on — for targeted requests.
    pub fn get_fdp(&self, id: &str) -> Option<(FdpInfo, LocalInterface)> {
        let st = self.state();
        let entry = st.cameras.get(id)?;
        Some((entry.camera.fdp.clone()?, entry.meta.as_ref()?.local.clone()))
    }

    /// Run a discovery cycle after `delay`, e.g. after an IP change.
    pub fn rediscover(&self, delay: Duration) {
        let s = self.clone();
        self.spawn(async move {
            sleep(delay).await;
            s.run_discovery();
        });
    }

    /// Re-evaluate the key trust state after the user trusted a key.
    pub fn update_key_trust(&self, id: &str) {
        let camera = {
            let mut st = self.state();
            let Some(entry) = st.cameras.get_mut(id) else { return };
            let Some(info) = entry.camera.fdp.as_mut() else { return };
            info.key_trust =
                self.inner.trust.lock().unwrap_or_else(|e| e.into_inner()).check(&info.serial, &info.key_fp);
            entry.camera.clone()
        };
        self.emit_all(vec![Event::Updated(camera)]);
    }

    fn run_discovery(&self) {
        self.discover_fdp();

        let (tx, mut rx) = mpsc::unbounded_channel();
        self.track(mdns::browse(tx));
        let s = self.clone();
        self.track(async move {
            while let Some((hostname, ip)) = rx.recv().await {
                match ip {
                    Some(ip) => s.add_camera(&hostname, ip),
                    None => {
                        let task = s.clone();
                        s.track(async move {
                            if let Some(ip) = mdns::resolve(&hostname).await {
                                task.add_camera(&hostname, ip);
                            }
                        });
                    }
                }
            }
        });
    }

    // --- Discovery via FDP (discovery-protocol.md) ---

    fn discover_fdp(&self) {
        {
            let mut st = self.state();
            if st.fdp_running {
                return;
            }
            st.fdp_running = true;
        }
        let s = self.clone();
        self.track(async move {
            // Check the web UI before showing the result: a camera in another
            // subnet may well be reachable through a router, so the address
            // alone must not decide the status (spec §8.3)
            let checks: Vec<_> = client::discover()
                .await
                .into_iter()
                .map(|result| {
                    let s = s.clone();
                    // Runs inside a runtime task, so tokio::spawn finds the runtime
                    tokio::spawn(async move {
                        let reachable = match &result.message.body {
                            Body::Announce { device, net, .. } => match net.address.as_deref() {
                                Some(address) => probe(fdp::strip_prefix(address), device.web.port).await,
                                None => false,
                            },
                            _ => false,
                        };
                        s.add_fdp_camera(result, reachable);
                    })
                })
                .collect();
            for check in checks {
                let _ = check.await;
            }
            s.state().fdp_running = false;
        });
    }

    /// Add or update an FDP camera; `reachable` is the result of the TCP check.
    fn add_fdp_camera(&self, discovered: Discovered, reachable: bool) {
        let Body::Announce { device, net, caps, set_ip, versions } = discovered.message.body else { return };
        let id = format!("sn-{}", device.serial);
        let ip = net.address.as_deref().map(fdp::strip_prefix).unwrap_or_default().to_string();
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
                .filter(|e| e.camera.fdp.is_none() && !ip.is_empty() && e.camera.ip == ip)
                .map(|e| e.camera.id.clone())
                .collect();
            for other in legacy {
                st.cameras.remove(&other);
                events.push(Event::Removed(other));
            }

            let info = FdpInfo {
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
                version: fdp::negotiate_version(versions.as_deref()),
                versions: versions.unwrap_or_else(|| vec![1]),
                local_address: discovered.local.address.to_string(),
                local_cidr: discovered.local.cidr.clone(),
            };

            let existing = st.cameras.get(&id).map(|e| e.camera.last_seen);
            let meta = FdpMeta { local: discovered.local, last_announce: Instant::now() };
            let mut camera = Camera {
                id: id.clone(),
                hostname: device.hostname,
                ip,
                port: device.web.port,
                scheme: device.web.scheme,
                last_seen: if reachable { now_ms() } else { existing.unwrap_or_else(now_ms) },
                online: reachable,
                status: CameraStatus::Offline,
                fdp: Some(info),
            };
            camera.status = status_for(&camera, Some(&meta), None, reachable);

            events.push(if existing.is_some() { Event::Updated(camera.clone()) } else { Event::Found(camera.clone()) });
            st.insert(camera, Some(meta), None);
        }

        self.emit_all(events);
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
            fdp: None,
        };

        {
            let mut st = self.state();
            // Already known via FDP — that entry wins (spec §9)
            if st.cameras.values().any(|e| e.camera.fdp.is_some() && e.camera.ip == ip) {
                return;
            }
            if let Some(entry) = st.cameras.get_mut(&id) {
                // Already known — note that it is still announced, verify the web UI
                entry.last_mdns = Some(Instant::now());
                drop(st);
                return self.check_camera(&id);
            }
            if st.dismissed.contains(&id) {
                // Dismissed by refresh — silently check if it's back online
                drop(st);
                return self.check_camera_dismissed(new_camera());
            }
        }

        // New camera — check the web UI first, then show it (like FDP cameras)
        let mut camera = new_camera();
        let s = self.clone();
        self.track(async move {
            let reachable = probe(&camera.ip, camera.port).await;
            let seen = Instant::now();
            camera.online = reachable;
            camera.status = status_for(&camera, None, Some(seen), reachable);
            s.state().insert(camera.clone(), None, Some(seen));
            s.emit_all(vec![Event::Found(camera)]);
        });
    }

    /// Check a dismissed camera — only re-add it if it's actually reachable.
    fn check_camera_dismissed(&self, mut camera: Camera) {
        let s = self.clone();
        self.track(async move {
            if !probe(&camera.ip, camera.port).await {
                return;
            }
            camera.online = true;
            camera.status = CameraStatus::Online;
            camera.last_seen = now_ms();
            {
                let mut st = s.state();
                st.dismissed.remove(&camera.id);
                st.insert(camera.clone(), None, Some(Instant::now()));
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
        self.track(async move {
            let reachable = probe(&ip, port).await;
            let updated = {
                let mut st = s.state();
                // Ignore results for entries that were replaced or removed meanwhile
                let Some(entry) = st.cameras.get_mut(&id).filter(|e| e.generation == generation) else { return };
                if reachable {
                    entry.camera.last_seen = now_ms();
                }
                let status = status_for(&entry.camera, entry.meta.as_ref(), entry.last_mdns, reachable);
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
