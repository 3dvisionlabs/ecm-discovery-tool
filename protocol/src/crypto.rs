//! FDP cryptography (spec §5): X25519 + HKDF-SHA256 + AES-256-GCM.

use aes_gcm::aead::{Aead, KeyInit, Payload};
use aes_gcm::{Aes256Gcm, Nonce};
use base64::Engine;
use base64::engine::general_purpose::STANDARD_NO_PAD;
use hkdf::Hkdf;
use sha2::{Digest, Sha256};
use x25519_dalek::{PublicKey, StaticSecret};

use crate::message::Sealed;
use crate::{ENC_ALG, hkdf_info};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Error(pub String);

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for Error {}

fn err(msg: &str) -> Error {
    Error(msg.to_string())
}

#[derive(Clone)]
pub struct KeyPair {
    pub private: [u8; 32],
    pub public: [u8; 32],
}

pub fn key_pair_from_private(private: [u8; 32]) -> KeyPair {
    let public = PublicKey::from(&StaticSecret::from(private)).to_bytes();
    KeyPair { private, public }
}

pub fn generate_key_pair() -> KeyPair {
    let mut private = [0u8; 32];
    getrandom::fill(&mut private).expect("OS random number generator unavailable");
    key_pair_from_private(private)
}

/// OpenSSH-style fingerprint: "SHA256:" + unpadded base64 (spec §5.1).
pub fn fingerprint(public: &[u8]) -> String {
    format!("SHA256:{}", STANDARD_NO_PAD.encode(Sha256::digest(public)))
}

fn derive_key(
    version: u32,
    private: &[u8; 32],
    peer: &[u8; 32],
    epk: &[u8; 32],
    device_pub: &[u8; 32],
) -> Result<[u8; 32], Error> {
    let shared = StaticSecret::from(*private).diffie_hellman(&PublicKey::from(*peer));
    if !shared.was_contributory() {
        return Err(err("X25519 produced an all-zero shared secret"));
    }
    let salt = [epk.as_slice(), device_pub.as_slice()].concat();
    let mut key = [0u8; 32];
    Hkdf::<Sha256>::new(Some(&salt), shared.as_bytes())
        .expand(hkdf_info(version).as_bytes(), &mut key)
        .map_err(|_| err("HKDF expand failed"))?;
    Ok(key)
}

/// Fixed values for test vectors only.
#[derive(Default)]
pub struct SealOptions {
    pub ephemeral_private: Option<[u8; 32]>,
    pub nonce: Option<[u8; 12]>,
}

/// Encrypt `plaintext` for the device public key. `aad_for` builds the AAD
/// from the ephemeral public key (hex), because the AAD includes it. The
/// protocol version is bound into the key derivation.
pub fn seal(
    version: u32,
    device_pub: &[u8; 32],
    plaintext: &[u8],
    aad_for: impl FnOnce(&str) -> String,
    opts: SealOptions,
) -> Result<Sealed, Error> {
    let eph = match opts.ephemeral_private {
        Some(private) => key_pair_from_private(private),
        None => generate_key_pair(),
    };
    let key = derive_key(version, &eph.private, device_pub, &eph.public, device_pub)?;
    let nonce = opts.nonce.unwrap_or_else(|| {
        let mut n = [0u8; 12];
        getrandom::fill(&mut n).expect("OS random number generator unavailable");
        n
    });
    let epk_hex = hex::encode(eph.public);
    let aad = aad_for(&epk_hex);

    let cipher = Aes256Gcm::new_from_slice(&key).map_err(|_| err("invalid key length"))?;
    let ct = cipher
        .encrypt(&Nonce::from(nonce), Payload { msg: plaintext, aad: aad.as_bytes() })
        .map_err(|_| err("encryption failed"))?;

    Ok(Sealed { alg: ENC_ALG.into(), epk: epk_hex, nonce: hex::encode(nonce), ct: hex::encode(ct) })
}

/// Decrypt a sealed payload with the device key pair (firmware side, tests).
pub fn open(version: u32, device: &KeyPair, sealed: &Sealed, aad: &str) -> Result<Vec<u8>, Error> {
    if sealed.alg != ENC_ALG {
        return Err(Error(format!("unsupported enc.alg {}", sealed.alg)));
    }
    let malformed = || err("malformed payload");
    let epk: [u8; 32] = hex::decode(&sealed.epk).ok().and_then(|v| v.try_into().ok()).ok_or_else(malformed)?;
    let nonce: [u8; 12] = hex::decode(&sealed.nonce).ok().and_then(|v| v.try_into().ok()).ok_or_else(malformed)?;
    let ct = hex::decode(&sealed.ct).map_err(|_| malformed())?;
    if ct.len() < 16 {
        return Err(malformed());
    }

    let key = derive_key(version, &device.private, &epk, &epk, &device.public)?;
    let cipher = Aes256Gcm::new_from_slice(&key).map_err(|_| err("invalid key length"))?;
    cipher.decrypt(&Nonce::from(nonce), Payload { msg: &ct, aad: aad.as_bytes() }).map_err(|_| err("decryption failed"))
}
