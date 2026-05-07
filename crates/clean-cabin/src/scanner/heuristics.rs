//! Rule-based heuristic scoring of file entries.

use std::time::SystemTime;

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
pub fn score(entry: &FileEntry, config: &Config) -> HeuristicScore {
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
