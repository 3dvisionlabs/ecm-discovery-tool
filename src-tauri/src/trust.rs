//! Trust-on-first-use store for ECMDP device keys (spec §8.5).
//! Maps camera serial number → trusted key fingerprint, persisted as JSON.

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::time::SystemTime;

use serde::{Deserialize, Serialize};

use crate::types::KeyTrust;

#[derive(Serialize, Deserialize)]
struct Entry {
    fp: String,
    trusted_at: String,
}

pub struct TrustStore {
    file: PathBuf,
    entries: BTreeMap<String, Entry>,
}

impl TrustStore {
    pub fn load(file: PathBuf) -> Self {
        // Missing or unreadable file: start empty
        let entries = std::fs::read(&file).ok().and_then(|data| serde_json::from_slice(&data).ok()).unwrap_or_default();
        TrustStore { file, entries }
    }

    pub fn get(&self, serial: &str) -> Option<String> {
        self.entries.get(serial).map(|e| e.fp.clone())
    }

    pub fn check(&self, serial: &str, fp: &str) -> KeyTrust {
        match self.entries.get(serial) {
            None => KeyTrust::New,
            Some(e) if e.fp == fp => KeyTrust::Match,
            Some(_) => KeyTrust::Mismatch,
        }
    }

    pub fn trust(&mut self, serial: &str, fp: &str) {
        let trusted_at = humantime::format_rfc3339_millis(SystemTime::now()).to_string();
        self.entries.insert(serial.to_string(), Entry { fp: fp.to_string(), trusted_at });
        self.save();
    }

    fn save(&self) {
        let result = self
            .file
            .parent()
            .map_or(Ok(()), std::fs::create_dir_all)
            .and_then(|_| std::fs::write(&self.file, serde_json::to_vec_pretty(&self.entries).unwrap_or_default()));
        if let Err(err) = result {
            eprintln!("Failed to save trusted camera keys: {err}");
        }
    }
}
