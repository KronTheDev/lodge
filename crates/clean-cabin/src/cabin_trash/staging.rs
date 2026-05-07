//! Staging area management — move, list, recover, and purge staged files.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use chrono::{NaiveDate, Utc};

use crate::report::selector::FlaggedFile;

/// A single file recorded in a staging manifest.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct StagedFile {
    /// Original absolute path before staging.
    pub original_path: PathBuf,
    /// Filename as stored inside the session directory.
    pub staged_name: String,
    /// File size in bytes at the time of staging.
    pub size: u64,
    /// Tier label ("clear_out", "worth_a_look", "you_decide").
    pub tier: String,
    /// Human-readable reason for flagging.
    pub reason: String,
    /// ISO 8601 timestamp when the file was staged.
    pub staged_at: String,
}

/// The manifest written for a staging session.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct StagingManifest {
    /// Session identifier (YYYY-MM-DD_HHMMSS).
    pub session_id: String,
    /// All files recorded in this session.
    pub files: Vec<StagedFile>,
}

/// Returns the root of the staging area.
pub fn staging_root() -> PathBuf {
    #[cfg(windows)]
    {
        std::env::var_os("LOCALAPPDATA")
            .map(|b| PathBuf::from(b).join(".cabin-trash"))
            .unwrap_or_else(|| PathBuf::from(".cabin-trash"))
    }
    #[cfg(not(windows))]
    {
        std::env::var_os("HOME")
            .map(|h| {
                PathBuf::from(h)
                    .join(".local")
                    .join("share")
                    .join("lodge")
                    .join(".cabin-trash")
            })
            .unwrap_or_else(|| PathBuf::from(".cabin-trash"))
    }
}

/// Build the list of `StagedFile` entries from flagged files (shared by both
/// `stage_files` and `init_session`).
fn build_staged_list(files: &[&FlaggedFile]) -> Vec<StagedFile> {
    let now = Utc::now().to_rfc3339();

    files
        .iter()
        .enumerate()
        .map(|(i, f)| {
            let ext = f
                .entry
                .path
                .extension()
                .map(|e| format!(".{}", e.to_string_lossy()))
                .unwrap_or_default();
            // Disambiguate filenames with an index prefix.
            let staged_name = format!(
                "{:04}_{}{ext}",
                i,
                f.entry
                    .path
                    .file_stem()
                    .map(|s| s.to_string_lossy().to_string())
                    .unwrap_or_else(|| "file".into())
            );

            let tier = match f.tier {
                crate::report::tiers::Tier::ClearOut => "clear_out",
                crate::report::tiers::Tier::WorthALook => "worth_a_look",
                crate::report::tiers::Tier::YouDecide => "you_decide",
            };

            StagedFile {
                original_path: f.entry.path.clone(),
                staged_name,
                size: f.entry.size,
                tier: tier.to_string(),
                reason: f.reason.clone(),
                staged_at: now.clone(),
            }
        })
        .collect()
}

/// Create the session directory and write the manifest.
///
/// Returns `(session_dir, manifest)`. Must be called before any
/// `stage_one_file()` calls. The manifest is written before any files are
/// moved, so recovery is always possible even if the process is interrupted.
pub fn init_session(files: &[&FlaggedFile], session_id: &str) -> Result<(PathBuf, StagingManifest)> {
    let session_dir = staging_root().join(session_id);
    let files_dir = session_dir.join("files");

    std::fs::create_dir_all(&files_dir)
        .with_context(|| format!("couldn't create staging directory {}", files_dir.display()))?;

    let staged = build_staged_list(files);

    let manifest = StagingManifest {
        session_id: session_id.to_string(),
        files: staged,
    };

    // Write manifest before any file moves.
    let manifest_path = session_dir.join("manifest.json");
    let manifest_json =
        serde_json::to_string_pretty(&manifest).context("couldn't serialise staging manifest")?;
    std::fs::write(&manifest_path, manifest_json)
        .with_context(|| format!("couldn't write manifest {}", manifest_path.display()))?;

    Ok((session_dir, manifest))
}

/// Stage a single file given its entry in the manifest.
///
/// Moves `staged.original_path` to `session_dir/files/staged.staged_name`.
/// Returns the number of bytes freed (the file's size on disk before the move).
pub fn stage_one_file(session_dir: &Path, staged: &StagedFile) -> Result<u64> {
    let files_dir = session_dir.join("files");
    let dest = files_dir.join(&staged.staged_name);

    let bytes = staged.size;

    if let Err(e) = std::fs::rename(&staged.original_path, &dest) {
        // Cross-device move — fall back to copy + delete.
        if e.kind() == std::io::ErrorKind::CrossesDevices
            || e.raw_os_error() == Some(17) // EXDEV on Linux
        {
            std::fs::copy(&staged.original_path, &dest).with_context(|| {
                format!(
                    "couldn't copy {} to {}",
                    staged.original_path.display(),
                    dest.display()
                )
            })?;
            std::fs::remove_file(&staged.original_path).with_context(|| {
                format!("couldn't remove original {}", staged.original_path.display())
            })?;
        }
        // For other errors (file may already be gone), continue silently.
    }

    Ok(bytes)
}

/// Move selected files to `staging_root()/<session_id>/files/` in a single call.
///
/// Convenience wrapper around `init_session` + `stage_one_file`. Prefer the
/// two-step API for live TUI progress.
///
/// The manifest is written **before** any files are moved, so recovery is
/// always possible even if the process is interrupted mid-way.
#[allow(dead_code)]
pub fn stage_files(files: &[&FlaggedFile], session_id: &str) -> Result<StagingManifest> {
    let (session_dir, manifest) = init_session(files, session_id)?;

    // Move each file.
    for sf in &manifest.files {
        let _ = stage_one_file(&session_dir, sf);
    }

    Ok(manifest)
}

/// List all staging sessions found under `staging_root()`.
pub fn list_sessions() -> Vec<(String, StagingManifest)> {
    let root = staging_root();
    let mut sessions = Vec::new();

    let read_dir = match std::fs::read_dir(&root) {
        Ok(d) => d,
        Err(_) => return sessions,
    };

    for entry in read_dir.flatten() {
        let session_id = entry.file_name().to_string_lossy().to_string();
        let manifest_path = entry.path().join("manifest.json");

        if let Ok(contents) = std::fs::read_to_string(&manifest_path) {
            if let Ok(manifest) = serde_json::from_str::<StagingManifest>(&contents) {
                sessions.push((session_id, manifest));
            }
        }
    }

    sessions.sort_by(|a, b| a.0.cmp(&b.0));
    sessions
}

/// Restore all files from a staging session to their original paths.
///
/// Returns the count of files successfully restored.
pub fn recover_session(session_id: &str) -> Result<usize> {
    let session_dir = staging_root().join(session_id);
    let manifest_path = session_dir.join("manifest.json");

    let contents =
        std::fs::read_to_string(&manifest_path).with_context(|| {
            format!("couldn't read manifest for session '{session_id}'")
        })?;
    let manifest: StagingManifest =
        serde_json::from_str(&contents).context("couldn't parse session manifest")?;

    let files_dir = session_dir.join("files");
    let mut count = 0usize;

    for sf in &manifest.files {
        let staged = files_dir.join(&sf.staged_name);

        // Ensure the parent directory exists.
        if let Some(parent) = sf.original_path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }

        if std::fs::rename(&staged, &sf.original_path).is_ok() {
            count += 1;
        } else {
            // Cross-device fallback.
            if std::fs::copy(&staged, &sf.original_path).is_ok() {
                let _ = std::fs::remove_file(&staged);
                count += 1;
            }
        }
    }

    Ok(count)
}

/// Permanently delete all staged files for a session and remove the session directory.
///
/// Returns the total bytes freed.
pub fn purge_session(session_id: &str) -> Result<u64> {
    let session_dir = staging_root().join(session_id);
    let manifest_path = session_dir.join("manifest.json");

    let mut freed = 0u64;

    if let Ok(contents) = std::fs::read_to_string(&manifest_path) {
        if let Ok(manifest) = serde_json::from_str::<StagingManifest>(&contents) {
            for sf in &manifest.files {
                let staged = session_dir.join("files").join(&sf.staged_name);
                if let Ok(meta) = std::fs::metadata(&staged) {
                    freed += meta.len();
                }
                let _ = std::fs::remove_file(&staged);
            }
        }
    }

    // Remove the whole session directory.
    let _ = std::fs::remove_dir_all(&session_dir);

    Ok(freed)
}

/// Purge all sessions whose session_id date is before `cutoff`.
///
/// Returns `(sessions_purged, bytes_freed)`.
pub fn purge_before(cutoff: NaiveDate) -> Result<(usize, u64)> {
    let sessions = list_sessions();
    let mut count = 0usize;
    let mut freed = 0u64;

    for (session_id, _) in sessions {
        // Session IDs are formatted as "YYYY-MM-DD_HHMMSS".
        if let Some(date_part) = session_id.split('_').next() {
            if let Ok(date) = NaiveDate::parse_from_str(date_part, "%Y-%m-%d") {
                if date < cutoff {
                    freed += purge_session(&session_id).unwrap_or(0);
                    count += 1;
                }
            }
        }
    }

    Ok((count, freed))
}

/// Silently purge sessions older than `retention_days` days.
///
/// Called at startup; errors are discarded.
pub fn auto_purge(retention_days: u32) {
    let cutoff = Utc::now()
        .date_naive()
        .checked_sub_days(chrono::Days::new(retention_days as u64))
        .unwrap_or_else(|| Utc::now().date_naive());

    let _ = purge_before(cutoff);
}

/// Generate a session ID in the format `YYYY-MM-DD_HHMMSS`.
pub fn new_session_id() -> String {
    Utc::now().format("%Y-%m-%d_%H%M%S").to_string()
}

/// Return the date on which a session will be auto-purged.
pub fn expiry_date(session_id: &str, retention_days: u32) -> Option<NaiveDate> {
    let date_part = session_id.split('_').next()?;
    let date = NaiveDate::parse_from_str(date_part, "%Y-%m-%d").ok()?;
    date.checked_add_days(chrono::Days::new(retention_days as u64))
}
