//! SET_IP and IDENTIFY flows (spec §8.4, §8.5).

use std::collections::HashMap;
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, Instant};

use ecmdp::crypto::{self, SealOptions};
use ecmdp::{Body, Message, SetIpPayload, Target};
use zeroize::Zeroize;

use crate::client::{self, LocalInterface};
use crate::messages::{status_message, version_message};
use crate::scanner::Scanner;
use crate::trust::TrustStore;
use crate::types::{IdentifyResult, KeyTrust, PrepareSetIpResult, SetIpRequest, SetIpResult};

/// Challenges are valid for 30 s on the device (spec §4.2); renew a bit earlier.
const CHALLENGE_MAX_AGE: Duration = Duration::from_secs(25);
/// Re-discover after a successful SET_IP (spec §8.4)
const REDISCOVER_DELAY: Duration = Duration::from_secs(3);
const IDENTIFY_DURATION: u32 = 10;

struct PendingChallenge {
    challenge: String,
    public: [u8; 32],
    fp: String,
    received_at: Instant,
}

/// Failure while getting a challenge: status for the SET_IP result and a message.
struct ChallengeError {
    status: String,
    message: String,
}

impl ChallengeError {
    fn new(status: &str, message: String) -> Self {
        ChallengeError { status: status.into(), message }
    }
}

fn unexpected(err: std::io::Error) -> String {
    format!("Unexpected error: {err}")
}

pub struct Actions {
    scanner: Scanner,
    trust: Arc<Mutex<TrustStore>>,
    pending: Mutex<HashMap<String, PendingChallenge>>,
}

impl Actions {
    pub fn new(scanner: Scanner, trust: Arc<Mutex<TrustStore>>) -> Self {
        Actions { scanner, trust, pending: Mutex::default() }
    }

    fn trust(&self) -> MutexGuard<'_, TrustStore> {
        self.trust.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn pending(&self) -> MutexGuard<'_, HashMap<String, PendingChallenge>> {
        self.pending.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Get a challenge and the device key, and check it against the trust store.
    pub async fn prepare_set_ip(&self, id: &str) -> PrepareSetIpResult {
        let Some((info, local)) = self.scanner.get_ecmdp(id) else {
            return PrepareSetIpResult::error("Camera not found.");
        };
        let Some(version) = info.version else {
            return PrepareSetIpResult::error(version_message(&info.versions));
        };

        let target = Target { serial: info.serial.clone(), mac: info.mac.clone() };
        let pending = match self.request_challenge(&local, version, target).await {
            Ok(pending) => pending,
            Err(e) => return PrepareSetIpResult::error(e.message),
        };

        let (trust, pinned) = {
            let store = self.trust();
            (store.check(&info.serial, &pending.fp), store.get(&info.serial))
        };
        let fingerprint = pending.fp.clone();
        self.pending().insert(id.to_string(), pending);
        PrepareSetIpResult {
            ok: true,
            error: None,
            fingerprint: Some(fingerprint),
            trust: Some(trust),
            pinned_fingerprint: pinned,
            suggestion: Some(ecmdp::suggest_config(&local.cidr, &info.net)),
        }
    }

    pub async fn set_ip(&self, mut req: SetIpRequest) -> SetIpResult {
        let result = self.send_set_ip(&req).await;
        req.password.zeroize();
        result
    }

    async fn send_set_ip(&self, req: &SetIpRequest) -> SetIpResult {
        let entry = self.scanner.get_ecmdp(&req.id);
        let prepared = self.pending().remove(&req.id);
        let (Some((info, local)), Some(prepared)) = (entry, prepared) else {
            return SetIpResult::new("error", "Camera not found. Please try again.");
        };
        let target = Target { serial: info.serial.clone(), mac: info.mac.clone() };
        let Some(version) = info.version else {
            return SetIpResult::new("unsupported_version", version_message(&info.versions));
        };

        if let Err(err) = ecmdp::validate_config(&req.config) {
            return SetIpResult::new("invalid_config", format!("The network settings are invalid: {err}"));
        }

        // Trust decision (spec §8.5): never send credentials to an untrusted key
        let trust_state = self.trust().check(&info.serial, &prepared.fp);
        if trust_state != KeyTrust::Match {
            if !req.accept_key {
                return SetIpResult::new("untrusted", status_message("untrusted", None, None));
            }
            self.trust().trust(&info.serial, &prepared.fp);
            self.scanner.update_key_trust(&req.id);
        }

        // The challenge may have expired while the user was typing
        let current = if prepared.received_at.elapsed() > CHALLENGE_MAX_AGE {
            match self.request_challenge(&local, version, target.clone()).await {
                Err(e) => return SetIpResult::new(&e.status, e.message),
                Ok(renewed) if renewed.fp != prepared.fp => {
                    return SetIpResult::new("key_changed", status_message("key_changed", None, None));
                }
                Ok(renewed) => renewed,
            }
        } else {
            prepared
        };

        let mut plaintext = serde_json::to_vec(&SetIpPayload {
            user: req.user.clone(),
            password: req.password.clone(),
            config: req.config.clone(),
        })
        .expect("payload serializes");
        let id = ecmdp::random_hex(16);
        let sealed = crypto::seal(
            version,
            &current.public,
            &plaintext,
            |epk| ecmdp::set_ip_aad(version, &target, &id, &current.challenge, epk),
            SealOptions::default(),
        );
        plaintext.zeroize();
        let enc = match sealed {
            Ok(enc) => enc,
            Err(err) => return SetIpResult::new("error", format!("Unexpected error: {err}")),
        };
        let msg = Message {
            proto: ecmdp::PROTO.into(),
            v: version,
            id,
            body: Body::SetIp { target, challenge: current.challenge, enc },
        };

        let res = match client::request(&local, &msg, &["set_ip_result"]).await {
            Err(err) => return SetIpResult::new("error", unexpected(err)),
            Ok(None) => return SetIpResult::new("timeout", status_message("timeout", None, None)),
            Ok(Some(res)) => res,
        };
        match res.body {
            Body::VersionError { versions } => SetIpResult::new("unsupported_version", version_message(&versions)),
            Body::SetIpResult { status, message, net } => {
                if status == "ok" {
                    self.scanner.rediscover(REDISCOVER_DELAY);
                }
                let text = status_message(&status, message.as_deref(), None);
                SetIpResult { status, message: Some(text), net }
            }
            _ => SetIpResult::new("error", "Unexpected response from the camera."),
        }
    }

    pub async fn identify(&self, id: &str) -> IdentifyResult {
        let Some((info, local)) = self.scanner.get_ecmdp(id) else {
            return IdentifyResult::failed("Camera not found.");
        };
        if !info.caps.iter().any(|c| c == "identify") {
            return IdentifyResult::failed("This camera does not support identify.");
        }
        let Some(version) = info.version else {
            return IdentifyResult::failed(version_message(&info.versions));
        };

        let msg = Message::new(
            version,
            Body::Identify { target: Target { serial: info.serial, mac: info.mac }, duration_s: IDENTIFY_DURATION },
        );
        let res = match client::request(&local, &msg, &["identify_result"]).await {
            Err(err) => return IdentifyResult::failed(unexpected(err)),
            Ok(None) => return IdentifyResult::failed(status_message("timeout", None, None)),
            Ok(Some(res)) => res,
        };
        match res.body {
            Body::VersionError { versions } => IdentifyResult::failed(version_message(&versions)),
            Body::IdentifyResult { status, .. } if status == "ok" => IdentifyResult {
                ok: true,
                message: format!("The camera LED is flashing for {IDENTIFY_DURATION} seconds."),
            },
            Body::IdentifyResult { message, .. } => {
                let detail = message.filter(|m| !m.is_empty()).map(|m| format!("\nCamera message: {m}"));
                IdentifyResult::failed(format!("The camera could not start identifying.{}", detail.unwrap_or_default()))
            }
            _ => IdentifyResult::failed("Unexpected response from the camera."),
        }
    }

    async fn request_challenge(
        &self,
        local: &LocalInterface,
        version: u32,
        target: Target,
    ) -> Result<PendingChallenge, ChallengeError> {
        let msg = Message::new(version, Body::ChallengeRequest { target });
        let res = client::request(local, &msg, &["challenge", "set_ip_result"])
            .await
            .map_err(|err| ChallengeError::new("error", unexpected(err)))?
            .ok_or_else(|| ChallengeError::new("timeout", status_message("timeout", None, None)))?;

        match res.body {
            Body::VersionError { versions } => {
                Err(ChallengeError::new("unsupported_version", version_message(&versions)))
            }
            // Device refuses right away, e.g. disabled or locked (spec §4.2)
            Body::SetIpResult { status, message, .. } => {
                let text = status_message(&status, message.as_deref(), None);
                Err(ChallengeError { status, message: text })
            }
            Body::Challenge { challenge, key } => {
                let public: Option<[u8; 32]> = hex::decode(&key.public).ok().and_then(|v| v.try_into().ok());
                let valid_challenge =
                    challenge.len() == 32 && challenge.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'));
                match public {
                    Some(public) if key.alg == "x25519" && valid_challenge => Ok(PendingChallenge {
                        challenge,
                        fp: crypto::fingerprint(&public),
                        public,
                        received_at: Instant::now(),
                    }),
                    _ => Err(ChallengeError::new(
                        "bad_request",
                        "The camera sent an invalid challenge. The app and the camera firmware probably implement \
                         different versions of the discovery protocol."
                            .into(),
                    )),
                }
            }
            _ => Err(ChallengeError::new("error", "Unexpected response from the camera.".into())),
        }
    }
}

/// End-to-end test against the simulated cameras in scripts/ecmdp-mock.ts,
/// an independent implementation of the device side. Run:
///   npm run mock:ecmdp -- --new-keys          (repository root)
///   ECMDP_LOOPBACK=1 cargo test -p ecm-discovery -- --ignored
#[cfg(test)]
mod mock_tests {
    use super::*;
    use crate::scanner::Event;
    use crate::types::CameraStatus;
    use ecmdp::{NetConfig, NetMode};

    fn request(id: &str, password: &str, accept_key: bool) -> SetIpRequest {
        SetIpRequest {
            id: id.into(),
            config: NetConfig {
                mode: NetMode::Static,
                address: Some("192.168.1.50/24".into()),
                gateway: None,
                dns: vec![],
                fallback: None,
            },
            user: "admin".into(),
            password: password.into(),
            accept_key,
        }
    }

    #[test]
    #[ignore = "needs the ECMDP mock (npm run mock:ecmdp)"]
    fn against_mock() {
        tauri::async_runtime::block_on(async {
            let dir = std::env::temp_dir().join(format!("ecm-trust-test-{}", ecmdp::random_hex(4)));
            let trust = Arc::new(Mutex::new(TrustStore::load(dir.join("trusted-cameras.json"))));
            let found = Arc::new(Mutex::new(Vec::new()));
            let events = found.clone();
            let scanner = Scanner::new(
                trust.clone(),
                Arc::new(move |e| {
                    if let Event::Found(c) = e {
                        events.lock().unwrap().push(c.id);
                    }
                }),
            );
            let actions = Actions::new(scanner.clone(), trust.clone());

            // Discovery takes 3 s, the TCP check of the reachable mock camera a bit more
            scanner.refresh();
            tokio::time::sleep(Duration::from_secs(5)).await;
            let cameras = scanner.get_all();
            let camera = |id: &str| cameras.iter().find(|c| c.id == id).unwrap_or_else(|| panic!("{id} not found"));
            for id in ["sn-244260001238", "sn-244260001377", "sn-232250000988", "sn-250110000042", "sn-231150000077"] {
                assert!(found.lock().unwrap().contains(&id.to_string()), "no found event for {id}");
            }
            assert_eq!(camera("sn-244260001377").hostname, "packaging-1");
            assert_eq!(camera("sn-244260001377").status, CameraStatus::Online);
            assert_eq!(camera("sn-231150000077").ecmdp.as_ref().unwrap().version, None);
            assert_eq!(camera("sn-244260001238").ecmdp.as_ref().unwrap().version, Some(1));

            // First contact: key must be confirmed before credentials are sent
            let id = "sn-244260001238";
            let prepared = actions.prepare_set_ip(id).await;
            assert!(prepared.ok, "{:?}", prepared.error);
            assert_eq!(prepared.trust, Some(KeyTrust::New));
            assert_eq!(prepared.fingerprint.as_ref().unwrap(), &camera(id).ecmdp.as_ref().unwrap().key_fp);
            assert_eq!(actions.set_ip(request(id, "3dvl", false)).await.status, "untrusted");

            // Wrong password: the key is trusted anyway, the camera refuses
            assert!(actions.prepare_set_ip(id).await.ok);
            assert_eq!(actions.set_ip(request(id, "wrong", true)).await.status, "auth_failed");

            let prepared = actions.prepare_set_ip(id).await;
            assert_eq!(prepared.trust, Some(KeyTrust::Match));
            let result = actions.set_ip(request(id, "3dvl", false)).await;
            assert_eq!(result.status, "ok", "{:?}", result.message);
            assert_eq!(result.net.unwrap().address.as_deref(), Some("192.168.1.50/24"));

            assert!(actions.identify(id).await.ok);
            let no_identify = actions.identify("sn-250110000042").await;
            assert_eq!(no_identify.message, "This camera does not support identify.");

            let disabled = actions.prepare_set_ip("sn-232250000988").await;
            assert!(disabled.error.unwrap().contains("disabled in the camera's web interface"));
            let incompatible = actions.prepare_set_ip("sn-231150000077").await;
            assert!(incompatible.error.unwrap().contains("Update this app."));

            let _ = std::fs::remove_dir_all(dir);
        });
    }
}
