//! Trust-on-first-use store for FDP device keys (spec §8.5).
//! Maps camera serial number → trusted key fingerprint, persisted as JSON.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
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

/// `<config dir>/Edge Camera Discovery/trusted-cameras.json`: the same file for
/// the app, the CLI and the Electron releases up to 0.9.1 (userData =
/// appData/<productName>), so trusted keys are shared and survive the switch.
pub fn default_file() -> Option<PathBuf> {
    Some(dirs::config_dir()?.join("Edge Camera Discovery").join("trusted-cameras.json"))
}

fn read(file: &Path) -> BTreeMap<String, Entry> {
    // Missing or unreadable file: start empty
    std::fs::read(file).ok().and_then(|data| serde_json::from_slice(&data).ok()).unwrap_or_default()
}

impl TrustStore {
    pub fn load(file: PathBuf) -> Self {
        let entries = read(&file);
        TrustStore { file, entries }
    }

    pub fn file(&self) -> &Path {
        &self.file
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
        // App and CLI may run at the same time: keep what the other one stored meanwhile
        self.entries = read(&self.file);
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
