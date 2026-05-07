//! Rule-based heuristic scoring of file entries.

use std::collections::HashMap;
use std::io::Read;
use std::path::PathBuf;
use std::time::SystemTime;

use sha2::{Digest, Sha256};

use crate::config::Config;
use crate::scanner::walker::FileEntry;

/// Coarse classification hint produced by heuristic scoring.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TierHint {
    /// Strongly suggest removal — junk, duplicates, obvious temps.
    ClearOut,
    /// Worth examining — large old files, probable installers.
    WorthALook,
    /// No strong signal either way.
    YouDecide,
    /// Keep — known system / app file; skip from report entirely.
    Keep,
}

/// Heuristic scoring result for a single file entry.
#[derive(Debug, Clone)]
pub struct HeuristicScore {
    /// Classification hint.
    pub tier_hint: TierHint,
    /// Human-readable reason for the classification.
    pub reason: String,
    /// Set by the receipt guard pass, not by heuristics themselves.
    pub is_receipt_guarded: bool,
}

/// Returns the age of `time` in whole days relative to now,
/// or `None` if `time` is in the future or unavailable.
fn age_days(time: Option<SystemTime>) -> Option<u64> {
    let t = time?;
    let duration = SystemTime::now().duration_since(t).ok()?;
    Some(duration.as_secs() / 86_400)
}

/// Compute the SHA-256 hex digest of a file.
fn sha256_file(path: &std::path::Path) -> Option<String> {
    let mut file = std::fs::File::open(path).ok()?;
    let mut hasher = Sha256::new();
    let mut buf = [0u8; 65536];
    loop {
        let n = file.read(&mut buf).ok()?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    Some(format!("{:x}", hasher.finalize()))
}

/// Known junk file names (exact, case-insensitive).
const JUNK_NAMES: &[&str] = &[
    "thumbs.db",
    ".ds_store",
    "desktop.ini",
    "._.ds_store",
];

/// Known junk name prefixes.
const JUNK_PREFIXES: &[&str] = &[
    "~$", // Office temporary files
];

/// Known junk extensions.
const JUNK_EXTENSIONS: &[&str] = &["tmp", "temp"];

/// Known installer name patterns (lowercased, partial).
const INSTALLER_PATTERNS: &[&str] = &[
    "setup_",
    "_installer",
    "uninstall",
    "_setup",
    "-setup",
    "-installer",
];

/// Known installer extensions.
const INSTALLER_EXTENSIONS: &[&str] = &["exe", "msi"];

/// Score a single `FileEntry` using rule-based heuristics.
///
/// `hashes` tracks SHA-256 → path for duplicate detection; entries are
/// inserted as files are scored so calling order matters.
pub fn score(
    entry: &FileEntry,
    config: &Config,
    hashes: &mut HashMap<String, PathBuf>,
) -> HeuristicScore {
    let name_lower = entry
        .path
        .file_name()
        .map(|n| n.to_string_lossy().to_lowercase())
        .unwrap_or_default();

    let ext_lower = entry
        .path
        .extension()
        .map(|e| e.to_string_lossy().to_lowercase())
        .unwrap_or_default();

    // ── Empty entries ────────────────────────────────────────────────────────
    if entry.is_empty {
        let label = if entry.is_dir {
            "empty directory"
        } else {
            "zero-byte file"
        };
        return HeuristicScore {
            tier_hint: TierHint::ClearOut,
            reason: label.into(),
            is_receipt_guarded: false,
        };
    }

    // ── Known junk names ─────────────────────────────────────────────────────
    if JUNK_NAMES.contains(&name_lower.as_str()) {
        return HeuristicScore {
            tier_hint: TierHint::ClearOut,
            reason: format!("{name_lower} — known system junk file"),
            is_receipt_guarded: false,
        };
    }

    // ── Known junk prefixes ───────────────────────────────────────────────────
    for prefix in JUNK_PREFIXES {
        if name_lower.starts_with(prefix) {
            return HeuristicScore {
                tier_hint: TierHint::ClearOut,
                reason: format!("temporary file ({name_lower})"),
                is_receipt_guarded: false,
            };
        }
    }

    // ── Known junk extensions ─────────────────────────────────────────────────
    if JUNK_EXTENSIONS.contains(&ext_lower.as_str()) {
        return HeuristicScore {
            tier_hint: TierHint::ClearOut,
            reason: format!(".{ext_lower} temporary file"),
            is_receipt_guarded: false,
        };
    }

    // ── Old log files ─────────────────────────────────────────────────────────
    if ext_lower == "log" {
        let age = age_days(entry.modified).unwrap_or(0);
        if age > 30 {
            return HeuristicScore {
                tier_hint: TierHint::ClearOut,
                reason: format!("log file not updated in {age} days"),
                is_receipt_guarded: false,
            };
        }
    }

    // ── Duplicate detection ───────────────────────────────────────────────────
    // Only hash files below a reasonable ceiling to avoid stalling on huge
    // binaries. Files ≥ 500 MB are skipped for duplicates.
    const HASH_SIZE_LIMIT: u64 = 500 * 1024 * 1024;
    if !entry.is_dir && entry.size > 0 && entry.size < HASH_SIZE_LIMIT {
        if let Some(digest) = sha256_file(&entry.path) {
            if let Some(original) = hashes.get(&digest) {
                let orig_name = original
                    .file_name()
                    .map(|n| n.to_string_lossy().to_string())
                    .unwrap_or_else(|| original.display().to_string());
                return HeuristicScore {
                    tier_hint: TierHint::ClearOut,
                    reason: format!("duplicate of {orig_name}"),
                    is_receipt_guarded: false,
                };
            }
            hashes.insert(digest, entry.path.clone());
        }
    }

    // ── Large, old files ─────────────────────────────────────────────────────
    let size_mb = entry.size / (1024 * 1024);
    let mod_age = age_days(entry.modified).unwrap_or(0);
    let acc_age = age_days(entry.accessed).unwrap_or(0);

    let min_age = u64::from(config.scan_min_age_days);
    if size_mb >= config.scan_large_file_mb
        && (mod_age >= min_age || acc_age >= min_age)
    {
        return HeuristicScore {
            tier_hint: TierHint::WorthALook,
            reason: format!(
                "{size_mb} MB — not accessed or modified in {} days",
                mod_age.max(acc_age)
            ),
            is_receipt_guarded: false,
        };
    }

    // ── Probable installer files ──────────────────────────────────────────────
    if INSTALLER_EXTENSIONS.contains(&ext_lower.as_str()) {
        let matches_pattern = INSTALLER_PATTERNS
            .iter()
            .any(|p| name_lower.contains(p));
        if matches_pattern {
            return HeuristicScore {
                tier_hint: TierHint::WorthALook,
                reason: "looks like an installer — may no longer be needed".into(),
                is_receipt_guarded: false,
            };
        }
    }

    // ── No signal ─────────────────────────────────────────────────────────────
    HeuristicScore {
        tier_hint: TierHint::YouDecide,
        reason: String::new(),
        is_receipt_guarded: false,
    }
}
