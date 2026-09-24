//! Test vectors from discovery-protocol.md §5.4 plus validation cases
//! (same cases as scripts/ecmdp-selftest.ts).

use ecmdp::crypto::{SealOptions, fingerprint, key_pair_from_private, open, seal};
use ecmdp::*;

fn hex32(s: &str) -> [u8; 32] {
    hex::decode(s).unwrap().try_into().unwrap()
}

fn device_key() -> crypto::KeyPair {
    key_pair_from_private(hex32(&format!("a8{}6b", "ab".repeat(30))))
}

fn target() -> Target {
    Target { serial: "244260001238".into(), mac: "00:1b:c5:0a:0b:0c".into() }
}

const ID: &str = "5f2d3c4b1a09f8e7d6c5b4a392817060";
const CHALLENGE: &str = "a1b2c3d4e5f60718293a4b5c6d7e8f90";
const PLAINTEXT: &str = r#"{"user":"admin","password":"3dvl","config":{"mode":"static","address":"169.254.10.20/16","gateway":null,"dns":[]}}"#;
const EXPECTED_CT: &str = "c45397007193e2340271cf5686f158690881dddaa511d4e73acb118555acef3833d837c2b2ff56bb70506a205df30879a6641cae723aab88f4e25a95dd86f5a23acebb1258cd2fa0bdb48646c998a61072c93e6a9a1895ddcee21922ef60a64e76c621ef7e307e413ec6ffa28520465d5f03ff28b4cc1e646e8b314f7af367cec28f";

fn sealed() -> Sealed {
    seal(
        1,
        &device_key().public,
        PLAINTEXT.as_bytes(),
        |epk| set_ip_aad(1, &target(), ID, CHALLENGE, epk),
        SealOptions {
            ephemeral_private: Some(hex32(&format!("c8{}4d", "cd".repeat(30)))),
            nonce: Some(hex::decode("000102030405060708090a0b").unwrap().try_into().unwrap()),
        },
    )
    .unwrap()
}

#[test]
fn device_public_key() {
    assert_eq!(hex::encode(device_key().public), "e3712d851a0e5d79b831c5e34ab22b41a198171de209b8b8faca23a11c624859");
}

#[test]
fn key_fingerprint() {
    assert_eq!(fingerprint(&device_key().public), "SHA256:vXOB6rCNLWJnXCfHnpmCFX5xYXoJWIZR2Ku6xQPTmSM");
}

#[test]
fn ephemeral_public_key() {
    assert_eq!(sealed().epk, "b5bea823d9c9ff576091c54b7c596c0ae296884f0e150290e88455d7fba6126f");
}

#[test]
fn ciphertext() {
    assert_eq!(sealed().ct, EXPECTED_CT);
}

#[test]
fn plaintext_matches_payload_serialization() {
    let payload = SetIpPayload {
        user: "admin".into(),
        password: "3dvl".into(),
        config: NetConfig {
            mode: NetMode::Static,
            address: Some("169.254.10.20/16".into()),
            gateway: None,
            dns: vec![],
            fallback: None,
        },
    };
    assert_eq!(serde_json::to_string(&payload).unwrap(), PLAINTEXT);
}

#[test]
fn decrypt_round_trip() {
    let s = sealed();
    let plain = open(1, &device_key(), &s, &set_ip_aad(1, &target(), ID, CHALLENGE, &s.epk)).unwrap();
    assert_eq!(plain, PLAINTEXT.as_bytes());
}

#[test]
fn decrypt_fails_with_other_challenge() {
    let s = sealed();
    assert!(open(1, &device_key(), &s, &set_ip_aad(1, &target(), ID, &"ff".repeat(16), &s.epk)).is_err());
}

#[test]
fn decrypt_fails_with_other_protocol_version() {
    let s = sealed();
    assert!(open(2, &device_key(), &s, &set_ip_aad(2, &target(), ID, CHALLENGE, &s.epk)).is_err());
}

#[test]
fn decrypt_fails_for_other_device() {
    let s = sealed();
    let other = Target { serial: "1".into(), ..target() };
    assert!(open(1, &device_key(), &s, &set_ip_aad(1, &other, ID, CHALLENGE, &s.epk)).is_err());
}

// --- Validation (spec §4.3.1) ---

fn config(mode: NetMode, address: Option<&str>, gateway: Option<&str>, dns: &[&str]) -> NetConfig {
    NetConfig {
        mode,
        address: address.map(Into::into),
        gateway: gateway.map(Into::into),
        dns: dns.iter().map(|s| s.to_string()).collect(),
        fallback: None,
    }
}

#[test]
fn valid_static_config() {
    assert_eq!(
        validate_config(&config(NetMode::Static, Some("192.168.1.50/24"), Some("192.168.1.1"), &["8.8.8.8"])),
        Ok(())
    );
}

#[test]
fn valid_dhcp_config_with_fallback() {
    let c = NetConfig {
        fallback: Some(StaticAddress { address: "192.168.1.100/24".into(), gateway: Some("192.168.1.1".into()) }),
        ..config(NetMode::Dhcp, None, None, &[])
    };
    assert_eq!(validate_config(&c), Ok(()));
}

#[test]
fn rejects_invalid_configs() {
    let bad = [
        config(NetMode::Static, Some("192.168.1.50"), None, &[]),
        config(NetMode::Static, Some("192.168.1.0/24"), None, &[]),
        config(NetMode::Static, Some("192.168.1.255/24"), None, &[]),
        config(NetMode::Static, Some("127.0.0.5/8"), None, &[]),
        config(NetMode::Static, Some("10.0.0.5/31"), None, &[]),
        config(NetMode::Static, Some("10.0.0.5/24"), Some("10.0.1.1"), &[]),
        config(NetMode::Static, Some("10.0.0.5/24"), None, &["1.1.1.1", "1.0.0.1", "8.8.8.8", "8.8.4.4"]),
        config(NetMode::Static, None, None, &[]),
    ];
    for c in bad {
        assert!(validate_config(&c).is_err(), "{c:?}");
    }
}

// --- Versioning (spec §3.2) ---

#[test]
fn version_negotiation() {
    assert_eq!(negotiate_version(Some(&[1])), Some(1));
    assert_eq!(negotiate_version(Some(&[1, 2])), Some(1));
    assert_eq!(negotiate_version(None), Some(1)); // no `versions` field = v1
    assert_eq!(negotiate_version(Some(&[2])), None);
}

// --- Helpers ---

#[test]
fn subnet_membership() {
    assert!(in_subnet("169.254.10.20", "169.254.33.7/16"));
    assert!(!in_subnet("192.168.1.100", "169.254.33.7/16"));
}

fn dhcp_state(address: &str) -> NetState {
    NetState {
        interface: "eth0".into(),
        mode: NetMode::Dhcp,
        address: Some(address.into()),
        gateway: None,
        dns: vec![],
        fallback: None,
    }
}

#[test]
fn suggest_link_local_for_apipa_client() {
    for _ in 0..100 {
        let s = suggest_config("169.254.33.7/16", &dhcp_state("192.168.1.100/24"));
        assert_eq!(validate_config(&s), Ok(()));
        assert!(in_subnet(strip_prefix(s.address.as_deref().unwrap()), "169.254.0.0/16"));
    }
}

#[test]
fn suggest_address_in_client_subnet() {
    for _ in 0..100 {
        let s = suggest_config("192.168.178.20/24", &dhcp_state("10.10.0.5/16"));
        assert_eq!(validate_config(&s), Ok(()));
        let address = s.address.as_deref().unwrap();
        assert!(in_subnet(strip_prefix(address), "192.168.178.0/24"));
        assert_ne!(address, "192.168.178.20/24");
    }
}

#[test]
fn suggest_dhcp_for_static_camera_in_normal_network() {
    let current = NetState { mode: NetMode::Static, ..dhcp_state("10.10.0.5/16") };
    assert_eq!(suggest_config("192.168.178.20/24", &current).mode, NetMode::Dhcp);
}

// --- Message format ---

#[test]
fn parses_announce_and_ignores_foreign_datagrams() {
    let announce = br#"{"proto":"ecmdp","v":1,"type":"announce","id":"ab","extra":1,
        "device":{"serial":"1","mac":"m","vendor":"3dvisionlabs","model":"C7","hostname":"ecm-1","fw_version":"3.1.0",
                  "web":{"scheme":"https","port":443},"key_fp":"SHA256:x"},
        "net":{"interface":"eth0","mode":"dhcp","address":"192.168.1.100/24","gateway":null,"dns":[],"fallback":null},
        "caps":["set_ip"],"set_ip":"enabled"}"#;
    let msg = Message::parse(announce).expect("announce parses");
    assert_eq!(msg.body.type_name(), "announce");
    assert!(Message::parse(br#"{"proto":"other","v":1,"type":"discover","id":"a"}"#).is_none());
    assert!(Message::parse(br#"{"proto":"ecmdp","v":0,"type":"discover","id":"a"}"#).is_none());
    assert!(Message::parse(br#"{"proto":"ecmdp","v":1,"type":"future_type","id":"a"}"#).is_none());
}

#[test]
fn serializes_header_and_type() {
    let msg = Message::new(1, Body::Discover {});
    let json: serde_json::Value = serde_json::from_slice(&msg.to_bytes()).unwrap();
    assert_eq!(json["proto"], "ecmdp");
    assert_eq!(json["type"], "discover");
    assert_eq!(json["id"].as_str().unwrap().len(), 32);
}
