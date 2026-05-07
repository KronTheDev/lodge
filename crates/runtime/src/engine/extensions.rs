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
//!
//! ## Registry schema versioning
//!
//! The `schema` field is a monotonic integer.  Lodge only accepts registries
//! whose `schema` value is ≤ `REGISTRY_SCHEMA`.  If a newer Lodge ships a
//! registry with a higher schema number, older installs fall back to their
//! local cache and display an "update Lodge" notice rather than silently
//! misinterpreting an unknown format.
//!
//! ## SHA-256 verification
//!
//! Each entry may carry a `sha256` field (lowercase hex, no prefix).  When
//! `download_extension` fetches a payload it verifies the digest and refuses
//! to write the file if the hash does not match.  Omitting the field skips
//! verification (useful for pre-release payloads where the hash isn't known
//! at registry-commit time), but a warning is displayed to the user.

use serde::{Deserialize, Serialize};

/// Highest registry schema version this build of Lodge understands.
pub const REGISTRY_SCHEMA: u32 = 1;

// ── Registry schema ───────────────────────────────────────────────────────────

/// A command entry for an extension, shown in the extension browser sub-pages.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExtCmd {
    /// Usage string, e.g. `"!clean <path>"`.
    pub usage: String,
    /// One-sentence description of what the command does.
    pub description: String,
}

/// A single extension in the registry.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RegistryEntry {
    pub id: String,
    pub name: String,
    pub version: String,
    pub description: String,
    /// "stable" | "preview" | "coming-soon"
    pub status: String,
    /// Short command alias used in the Lodge command bar (e.g. "clean" for "clean-cabin").
    /// If absent, the `id` is used as the command alias.
    #[serde(default)]
    pub alias: Option<String>,
    /// Local payload filename (zip), if the payload ships alongside the binary.
    #[serde(default)]
    pub payload: Option<String>,
    /// Remote download URL for the payload zip.
    #[serde(default)]
    pub payload_url: Option<String>,
    /// Lowercase hex SHA-256 of the payload zip.  `None` means unverified
    /// (allowed but warned).  Present means Lodge will reject a download whose
    /// digest does not match exactly.
    #[serde(default)]
    pub sha256: Option<String>,
    /// Optional list of sub-commands shown in the extension browser detail view.
    #[serde(default)]
    pub commands: Vec<ExtCmd>,
}

impl RegistryEntry {
    /// Returns the command alias — the short name the user types after `!`.
    pub fn command_alias(&self) -> &str {
        self.alias.as_deref().unwrap_or(&self.id)
    }
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
    let registry = Registry { schema: REGISTRY_SCHEMA, extensions: entries.to_vec() };
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

const REGISTRY_URL: &str =
    "https://raw.githubusercontent.com/KronTheDev/lodge/main/extensions/registry.json";

fn try_fetch() -> anyhow::Result<Vec<RegistryEntry>> {
    let response = reqwest::blocking::Client::builder()
        .timeout(std::time::Duration::from_secs(3))
        .build()?
        .get(REGISTRY_URL)
        .send()?;
    let registry: Registry = response.json()?;

    // Reject registries from a future Lodge version — fall back to cache so
    // the user sees their current extensions rather than a parse error.
    if registry.schema > REGISTRY_SCHEMA {
        anyhow::bail!(
            "registry schema {} is newer than this Lodge install supports (max {}). \
             update Lodge to get the latest extension list.",
            registry.schema,
            REGISTRY_SCHEMA
        );
    }

    Ok(registry.extensions)
}

// ── Payload download + verification ──────────────────────────────────────────

/// Download an extension payload from `entry.payload_url` into `dest_dir`.
///
/// Steps:
/// 1. Fetch the zip from `payload_url` (60 s timeout).
/// 2. If `entry.sha256` is set, verify the digest.  Mismatch → hard error.
///    If `sha256` is absent, the download proceeds but returns
///    `DownloadResult::UnverifiedOk` so the caller can warn the user.
/// 3. Write the zip to `dest_dir/<filename>` where `<filename>` is
///    `entry.payload` if set, otherwise the last path segment of the URL.
///
/// Returns the path to the written file.
pub fn download_extension(
    entry: &RegistryEntry,
    dest_dir: &std::path::Path,
) -> anyhow::Result<(std::path::PathBuf, bool)> {
    let url = entry
        .payload_url
        .as_deref()
        .filter(|u| !u.is_empty())
        .ok_or_else(|| anyhow::anyhow!("extension '{}' has no download URL", entry.id))?;

    // Determine the local filename.
    let filename: String = entry.payload.clone().unwrap_or_else(|| {
        url.rsplit('/').next().unwrap_or("extension.zip").to_string()
    });

    // Download.
    let response = reqwest::blocking::Client::builder()
        .timeout(std::time::Duration::from_secs(60))
        .build()?
        .get(url)
        .send()?;

    if !response.status().is_success() {
        anyhow::bail!(
            "download failed: HTTP {} — {}",
            response.status().as_u16(),
            url
        );
    }

    let bytes = response.bytes()?;

    // Verify SHA-256 if the registry entry carries one.
    let verified = if let Some(expected) = &entry.sha256 {
        if expected.is_empty() {
            false // treat empty string same as absent
        } else {
            use sha2::Digest;
            let mut hasher = sha2::Sha256::new();
            hasher.update(&bytes);
            let actual = hex_encode(&hasher.finalize());
            if actual != *expected {
                anyhow::bail!(
                    "checksum mismatch for '{}'\n  expected  {}\n  got       {}\n  \
                     the download has been discarded — do not install.",
                    entry.id, expected, actual
                );
            }
            true
        }
    } else {
        false // no hash in registry — unverified
    };

    // Write.
    std::fs::create_dir_all(dest_dir)?;
    let dest = dest_dir.join(&filename);
    std::fs::write(&dest, &bytes)?;

    // `verified` = false means "downloaded OK but no hash to check against".
    Ok((dest, verified))
}

fn hex_encode(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
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
