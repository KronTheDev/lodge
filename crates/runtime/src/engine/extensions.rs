//! Extension registry — fetches the public lodge registry from GitHub and
//! caches it locally so the list is always available offline.
//!
//! ## Contributing an extension
//!
//! Open a pull request against the Lodge repository that adds your manifest
//! entry to `extensions/registry.json`.  No other files need to change.
//! Lodge fetches this file on every session start and updates its local cache.
//!
//! ## Registry URL
//!
//! `https://raw.githubusercontent.com/KronTheDev/lodge/main/extensions/registry.json`

use serde::{Deserialize, Serialize};

const REGISTRY_URL: &str =
    "https://raw.githubusercontent.com/KronTheDev/lodge/main/extensions/registry.json";

// ── Registry schema ───────────────────────────────────────────────────────────

/// A single extension in the registry.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RegistryEntry {
    pub id: String,
    pub name: String,
    pub version: String,
    pub description: String,
    /// "stable" | "preview" | "coming-soon"
    pub status: String,
    /// Local payload filename (zip), if the payload ships alongside the binary.
    #[serde(default)]
    pub payload: Option<String>,
    /// Remote download URL for the payload zip (future use — currently optional).
    #[serde(default)]
    pub payload_url: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
struct Registry {
    schema: u32,
    extensions: Vec<RegistryEntry>,
}

// ── Cache path ────────────────────────────────────────────────────────────────

fn cache_path() -> Option<std::path::PathBuf> {
    #[cfg(windows)]
    let base = std::env::var_os("LOCALAPPDATA")?;
    #[cfg(not(windows))]
    let base = {
        let home = std::env::var_os("HOME")?;
        std::path::PathBuf::from(home)
            .join(".local")
            .join("share")
            .into_os_string()
    };
    Some(std::path::PathBuf::from(base).join("lodge").join("ext_cache.json"))
}

fn write_cache(entries: &[RegistryEntry]) {
    let Some(path) = cache_path() else { return };
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let registry = Registry { schema: 1, extensions: entries.to_vec() };
    if let Ok(json) = serde_json::to_string_pretty(&registry) {
        let _ = std::fs::write(path, json);
    }
}

fn read_cache() -> Option<Vec<RegistryEntry>> {
    let path = cache_path()?;
    let raw = std::fs::read_to_string(path).ok()?;
    let registry: Registry = serde_json::from_str(&raw).ok()?;
    Some(registry.extensions)
}

// ── Fetch ─────────────────────────────────────────────────────────────────────

/// Fetch the extension registry from GitHub and update the local cache.
///
/// Returns `(entries, online)`.  If the network fetch succeeds, `online` is
/// `true` and the cache is updated.  On failure, the cached copy is returned
/// with `online = false`.  If neither is available, returns an empty list.
///
/// Uses a 3-second timeout so it never hangs session start significantly.
pub fn fetch_registry() -> (Vec<RegistryEntry>, bool) {
    match try_fetch() {
        Ok(entries) => {
            write_cache(&entries);
            (entries, true)
        }
        Err(_) => {
            let cached = read_cache().unwrap_or_default();
            (cached, false)
        }
    }
}

fn try_fetch() -> anyhow::Result<Vec<RegistryEntry>> {
    let response = reqwest::blocking::Client::builder()
        .timeout(std::time::Duration::from_secs(3))
        .build()?
        .get(REGISTRY_URL)
        .send()?;
    let registry: Registry = response.json()?;
    Ok(registry.extensions)
}

// ── Local extensions directory (fallback / dev mode) ─────────────────────────

/// Returns the extensions directory: next to the binary in production,
/// or `./extensions/` when running via `cargo run`.
pub fn extensions_dir() -> std::path::PathBuf {
    if let Ok(exe) = std::env::current_exe() {
        let dir = exe
            .parent()
            .unwrap_or(std::path::Path::new("."))
            .join("extensions");
        if dir.is_dir() {
            return dir;
        }
    }
    std::path::PathBuf::from("extensions")
}
