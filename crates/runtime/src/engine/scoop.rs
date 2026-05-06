//! Scoop bucket integration — search, manifest fetch, download, and install.
//!
//! When a package is not in the local Lodge feed, the install flow falls back
//! to this provider. Buckets are probed in order until a match is found:
//!   1. Main   (ScoopInstaller/Main)
//!   2. Extras (ScoopInstaller/Extras)
//!
//! Only ZIP archives are fully supported. MSI/EXE/7z packages are rejected
//! with a clear error — redirect the user to install those manually or via
//! `winget` / `choco`.

use std::io::{Read, Write};
use std::path::{Path, PathBuf};

use anyhow::{bail, Context, Result};
use serde::Deserialize;
use sha2::{Digest, Sha256};

use lodge_shared::{
    manifest::{As, Manifest, PackageType, Prefers, Requires},
    placement::{PlacementEntry, PlacementPlan, RegistrationEffects},
    receipt::{PlacedFile, Receipt},
};

// ── Bucket registry ───────────────────────────────────────────────────────────

/// Buckets probed in order. Name is shown in the flashcard source line.
const BUCKETS: &[(&str, &str)] = &[
    ("main",   "ScoopInstaller/Main"),
    ("extras", "ScoopInstaller/Extras"),
];

const USER_AGENT: &str = concat!("lodge/", env!("CARGO_PKG_VERSION"));
const HTTP_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(20);

// ── Scoop manifest schema (subset) ───────────────────────────────────────────

/// A Scoop bucket manifest (only the fields Lodge needs).
///
/// Scoop's format is flexible — most fields can be a string, an array, or
/// absent. We use `serde_json::Value` for those and extract strings at runtime.
#[derive(Debug, Clone, Deserialize)]
pub struct ScoopManifest {
    pub version: String,
    pub description: Option<String>,
    pub homepage: Option<String>,

    /// Top-level URL (string or string array). May be overridden by `architecture`.
    pub url: Option<serde_json::Value>,
    /// Top-level hash (string or string array, matching `url`).
    pub hash: Option<serde_json::Value>,

    pub architecture: Option<ScoopArchitecture>,

    /// Binary names to expose as commands. String, string array, or array of
    /// `[path, alias]` pairs.
    pub bin: Option<serde_json::Value>,

    /// Sub-directory inside the extracted archive that contains the binaries.
    pub extract_dir: Option<serde_json::Value>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ScoopArchitecture {
    #[serde(rename = "64bit")]
    pub x64: Option<ScoopArch>,
    #[serde(rename = "arm64")]
    pub arm64: Option<ScoopArch>,
    // 32bit omitted — Lodge targets 64-bit only.
}

#[derive(Debug, Clone, Deserialize)]
pub struct ScoopArch {
    pub url: Option<serde_json::Value>,
    pub hash: Option<serde_json::Value>,
}

// ── Public result type ────────────────────────────────────────────────────────

/// A Scoop package ready for display or installation.
#[derive(Debug, Clone)]
pub struct ScoopPackage {
    /// Package id (the manifest filename without `.json`).
    pub id: String,
    /// Bucket this was found in (e.g. `"main"`).
    pub bucket: &'static str,
    pub manifest: ScoopManifest,
}

impl ScoopPackage {
    /// Build a synthetic Lodge [`Manifest`] for the flashcard.
    pub fn to_lodge_manifest(&self) -> Manifest {
        let bucket = self.bucket;
        Manifest {
            id: self.id.clone(),
            version: self.manifest.version.clone(),
            package_type: PackageType::CliTool,
            description: self.manifest.description.as_ref().map(|d| {
                format!("{d}  [scoop/{bucket}]")
            }),
            author: self.manifest.homepage.as_deref().map(homepage_to_author),
            prefers: Prefers::default(),
            requires: Requires::default(),
            naming: As::default(),
            overrides: vec![],
            hooks: Default::default(),
        }
    }

    /// Build a synthetic [`PlacementPlan`] for the flashcard destination preview.
    pub fn to_placement_plan(&self) -> PlacementPlan {
        let dest_dir = install_dir(&self.id);
        let bins = extract_bin_paths(&self.manifest);
        let entries = bins
            .iter()
            .map(|b| PlacementEntry {
                source: PathBuf::from(b),
                destination: dest_dir.join(std::path::Path::new(b).file_name().unwrap_or_default()),
                rename: None,
            })
            .collect();

        PlacementPlan {
            entries,
            registrations: RegistrationEffects {
                add_to_path: true,
                ..Default::default()
            },
            hooks_order: vec![],
            requires_elevation: false,
        }
    }
}

// ── Manifest fetching ─────────────────────────────────────────────────────────

/// Try to find `id` in the known Scoop buckets (first match wins).
pub fn fetch(id: &str) -> Result<ScoopPackage> {
    let client = blocking_client()?;
    for &(bucket_name, repo) in BUCKETS {
        let url = format!(
            "https://raw.githubusercontent.com/{repo}/master/bucket/{id}.json"
        );
        match client.get(&url).send() {
            Ok(resp) if resp.status().is_success() => {
                let manifest: ScoopManifest = resp
                    .json()
                    .with_context(|| format!("couldn't parse Scoop manifest for '{id}'"))?;
                return Ok(ScoopPackage { id: id.to_string(), bucket: bucket_name, manifest });
            }
            Ok(_) => continue,   // 404 or other non-success → try next bucket
            Err(e) => bail!("network error fetching Scoop manifest: {e}"),
        }
    }
    bail!("'{id}' not found in Scoop buckets (main, extras)")
}

// ── Installation ──────────────────────────────────────────────────────────────

/// Download, verify, extract, place, shim, and receipt a Scoop package.
///
/// Runs synchronously — call from a background thread.
pub fn install(pkg: &ScoopPackage, runtime_version: &str) -> Result<Receipt> {
    let id = &pkg.id;
    let manifest = &pkg.manifest;

    // Resolve download URL and expected hash for this architecture.
    let (url, expected_hash) = resolve_url_hash(manifest)
        .context("couldn't determine download URL from Scoop manifest")?;

    // Reject non-ZIP archives early with a clear message.
    let filename = url
        .rsplit('/')
        .next()
        .unwrap_or("package")
        .split('?')       // strip query strings
        .next()
        .unwrap_or("package");

    let lower = filename.to_lowercase();
    if !lower.ends_with(".zip") {
        // Non-ZIP (EXE, MSI, 7z, …) — delegate to an installed package manager.
        // winget is built into Windows 10 1709+ and handles most popular tools.
        // Scoop CLI is tried second. If neither is available, give a clear error.
        return try_delegate(id, &manifest.version, runtime_version);
    }

    // ── Download ─────────────────────────────────────────────────────────────
    let tmp = tempfile::tempdir().context("couldn't create temp directory")?;
    let archive_path = tmp.path().join(filename);

    download_file(&url, &archive_path)?;

    // ── Hash verification ─────────────────────────────────────────────────────
    if let Some(ref hash_str) = expected_hash {
        verify_hash(&archive_path, hash_str)
            .with_context(|| format!("hash verification failed for {id}"))?;
    }

    // ── Extraction ────────────────────────────────────────────────────────────
    let extract_root = tmp.path().join("extracted");
    std::fs::create_dir_all(&extract_root)?;
    extract_zip(&archive_path, &extract_root)
        .with_context(|| format!("couldn't extract {}", archive_path.display()))?;

    // Honour `extract_dir` (can be string or array — use first element).
    let content_dir = match first_str(&manifest.extract_dir) {
        Some(sub) => extract_root.join(&sub),
        None => {
            // If there's exactly one directory inside, use it; otherwise the root.
            let subdirs: Vec<_> = std::fs::read_dir(&extract_root)?
                .filter_map(|e| e.ok())
                .filter(|e| e.file_type().map(|t| t.is_dir()).unwrap_or(false))
                .collect();
            if subdirs.len() == 1 {
                subdirs[0].path()
            } else {
                extract_root.clone()
            }
        }
    };

    // ── Placement ─────────────────────────────────────────────────────────────
    let dest_dir = install_dir(id);
    std::fs::create_dir_all(&dest_dir)
        .with_context(|| format!("couldn't create install directory {}", dest_dir.display()))?;

    // Copy every file from content_dir to dest_dir (flat copy — preserves structure).
    let mut placed: Vec<PlacedFile> = Vec::new();
    copy_dir_recursive(&content_dir, &dest_dir, &mut placed)?;

    // ── Shims ─────────────────────────────────────────────────────────────────
    // Register a shim for each binary declared in `bin`.
    let bins = extract_bin_paths(manifest);
    let mut shim_names: Vec<String> = Vec::new();
    for bin_rel in &bins {
        let bin_path = dest_dir.join(std::path::Path::new(bin_rel).file_name().unwrap_or_default());
        if bin_path.exists() {
            let cmd_name = bin_path
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or(id);
            if crate::shim::register::register(cmd_name, &bin_path).is_ok() {
                shim_names.push(cmd_name.to_string());
            }
        }
    }

    // ── Receipt ───────────────────────────────────────────────────────────────
    let receipt = build_receipt(id, &manifest.version, placed, &shim_names, runtime_version)?;
    Ok(receipt)
}

// ── Helpers ───────────────────────────────────────────────────────────────────

/// Returns the Lodge install directory for a Scoop-sourced package.
fn install_dir(id: &str) -> PathBuf {
    #[cfg(windows)]
    {
        let base = std::env::var("LOCALAPPDATA").unwrap_or_else(|_| ".".into());
        PathBuf::from(base).join("Programs").join(id)
    }
    #[cfg(not(windows))]
    {
        let base = std::env::var("HOME").unwrap_or_else(|_| ".".into());
        PathBuf::from(base).join(".local").join("share").join(id)
    }
}

/// Resolve the download URL and hash appropriate for the current architecture.
fn resolve_url_hash(manifest: &ScoopManifest) -> Result<(String, Option<String>)> {
    // Prefer architecture-specific entry (arm64 first, then x64).
    if let Some(arch) = &manifest.architecture {
        // Try host arch (arm64 on ARM Windows, x64 otherwise)
        let preferred = if cfg!(target_arch = "aarch64") {
            arch.arm64.as_ref().or(arch.x64.as_ref())
        } else {
            arch.x64.as_ref()
        };
        if let Some(a) = preferred {
            if let Some(url) = first_str(&a.url) {
                return Ok((url, first_str(&a.hash)));
            }
        }
    }

    // Fall back to top-level URL.
    first_str(&manifest.url)
        .map(|url| (url, first_str(&manifest.hash)))
        .context("no download URL in Scoop manifest")
}

/// Extract the first string from a Value that might be a string or array.
fn first_str(val: &Option<serde_json::Value>) -> Option<String> {
    match val {
        Some(serde_json::Value::String(s)) => Some(s.clone()),
        Some(serde_json::Value::Array(arr)) => {
            arr.first().and_then(|v| v.as_str()).map(str::to_string)
        }
        _ => None,
    }
}

/// Extract binary relative paths from the `bin` field.
///
/// `bin` can be:
/// - `"tool.exe"`
/// - `["a.exe", "b.exe"]`
/// - `[["a.exe", "alias"], "b.exe"]`   (mixed)
fn extract_bin_paths(manifest: &ScoopManifest) -> Vec<String> {
    match &manifest.bin {
        None => vec![],
        Some(serde_json::Value::String(s)) => vec![s.clone()],
        Some(serde_json::Value::Array(arr)) => arr
            .iter()
            .filter_map(|v| match v {
                serde_json::Value::String(s) => Some(s.clone()),
                serde_json::Value::Array(inner) => {
                    // ["actual.exe", "alias"] — take the actual path
                    inner.first().and_then(|v| v.as_str()).map(str::to_string)
                }
                _ => None,
            })
            .collect(),
        _ => vec![],
    }
}

/// Build a reqwest blocking client with Lodge's user-agent and timeout.
fn blocking_client() -> Result<reqwest::blocking::Client> {
    reqwest::blocking::Client::builder()
        .user_agent(USER_AGENT)
        .timeout(HTTP_TIMEOUT)
        .build()
        .context("couldn't build HTTP client")
}

/// Stream-download `url` to `dest`, showing rough progress via byte count.
fn download_file(url: &str, dest: &Path) -> Result<()> {
    let client = blocking_client()?;
    let mut resp = client
        .get(url)
        .send()
        .with_context(|| format!("couldn't connect to {url}"))?;

    if !resp.status().is_success() {
        bail!("download failed: HTTP {}", resp.status());
    }

    let mut file = std::fs::File::create(dest)
        .with_context(|| format!("couldn't create temp file {}", dest.display()))?;

    let mut buf = [0u8; 64 * 1024];
    loop {
        let n = resp.read(&mut buf).context("download interrupted")?;
        if n == 0 {
            break;
        }
        file.write_all(&buf[..n]).context("write error during download")?;
    }
    Ok(())
}

/// Verify `file` against a Scoop hash string.
///
/// Scoop hashes are formatted as `"sha256:hexdigest"` or bare `"hexdigest"`.
fn verify_hash(file: &Path, hash_str: &str) -> Result<()> {
    let expected = hash_str
        .strip_prefix("sha256:")
        .unwrap_or(hash_str)
        .to_lowercase();

    let bytes = std::fs::read(file)
        .with_context(|| format!("couldn't read {} for verification", file.display()))?;

    let actual = format!("{:x}", Sha256::digest(&bytes));

    if actual != expected {
        bail!(
            "hash mismatch — expected {expected}, got {actual}. \
             the download may be corrupt or tampered with."
        );
    }
    Ok(())
}

/// Extract a ZIP archive to `dest_dir`.
fn extract_zip(archive: &Path, dest_dir: &Path) -> Result<()> {
    let file = std::fs::File::open(archive)
        .with_context(|| format!("couldn't open archive {}", archive.display()))?;
    let mut zip = zip::ZipArchive::new(file).context("not a valid ZIP archive")?;

    for i in 0..zip.len() {
        let mut entry = zip.by_index(i).context("couldn't read ZIP entry")?;
        let out_path = dest_dir.join(entry.mangled_name());

        if entry.is_dir() {
            std::fs::create_dir_all(&out_path)
                .with_context(|| format!("couldn't create {}", out_path.display()))?;
        } else {
            if let Some(parent) = out_path.parent() {
                std::fs::create_dir_all(parent)?;
            }
            let mut out = std::fs::File::create(&out_path)
                .with_context(|| format!("couldn't create {}", out_path.display()))?;
            std::io::copy(&mut entry, &mut out).context("error extracting file")?;
        }
    }
    Ok(())
}

/// Recursively copy all files from `src` to `dest`, recording placements.
fn copy_dir_recursive(src: &Path, dest: &Path, placed: &mut Vec<PlacedFile>) -> Result<()> {
    for entry in walkdir::WalkDir::new(src).min_depth(1) {
        let entry = entry.context("error walking extracted directory")?;
        let rel = entry.path().strip_prefix(src).expect("prefix is src");
        let target = dest.join(rel);

        if entry.file_type().is_dir() {
            std::fs::create_dir_all(&target)?;
        } else {
            if let Some(parent) = target.parent() {
                std::fs::create_dir_all(parent)?;
            }
            std::fs::copy(entry.path(), &target)
                .with_context(|| format!("couldn't copy {}", entry.path().display()))?;

            // Hash the placed file for the receipt.
            let bytes = std::fs::read(&target)?;
            placed.push(PlacedFile {
                source: entry.path().to_string_lossy().into_owned(),
                destination: target.to_string_lossy().into_owned(),
                hash: format!("{:x}", Sha256::digest(&bytes)),
            });
        }
    }
    Ok(())
}

/// Build and persist a Lodge receipt for a Scoop installation.
fn build_receipt(
    id: &str,
    version: &str,
    placements: Vec<PlacedFile>,
    shim_names: &[String],
    runtime_version: &str,
) -> Result<Receipt> {
    let installed_at = {
        #[cfg(windows)]
        {
            use std::time::{SystemTime, UNIX_EPOCH};
            let secs = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs();
            // Simple ISO-8601 UTC timestamp without chrono
            let s = secs;
            let (y, mo, d, h, mi, sec) = epoch_to_parts(s);
            format!("{y:04}-{mo:02}-{d:02}T{h:02}:{mi:02}:{sec:02}Z")
        }
        #[cfg(not(windows))]
        {
            use std::time::{SystemTime, UNIX_EPOCH};
            let secs = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs();
            let (y, mo, d, h, mi, sec) = epoch_to_parts(secs);
            format!("{y:04}-{mo:02}-{d:02}T{h:02}:{mi:02}:{sec:02}Z")
        }
    };

    let mut registrations = vec!["PATH".to_string()];
    registrations.extend(shim_names.iter().map(|s| format!("shim:{s}")));

    let mut receipt = Receipt {
        id: id.to_string(),
        version: version.to_string(),
        installed_at,
        scope: "user".to_string(),
        placements,
        registrations,
        hooks_run: vec![],
        runtime_version: runtime_version.to_string(),
        receipt_hash: String::new(),
    };

    // Sign the receipt.
    let json = serde_json::to_string(&receipt).context("couldn't serialise receipt")?;
    let hash = format!("{:x}", Sha256::digest(json.as_bytes()));
    receipt.receipt_hash = format!("sha256:{hash}");

    // Write to disk.
    let dir = crate::engine::attester::receipt_dir();
    std::fs::create_dir_all(&dir)?;

    let ts = {
        use std::time::{SystemTime, UNIX_EPOCH};
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs()
    };
    let path = dir.join(format!("{id}-{version}-{ts}.json"));
    let pretty = serde_json::to_string_pretty(&receipt).context("couldn't serialise receipt")?;
    std::fs::write(&path, pretty)
        .with_context(|| format!("couldn't write receipt to {}", path.display()))?;

    Ok(receipt)
}

/// Delegate installation to an available system package manager.
///
/// Called when the Scoop manifest specifies a non-ZIP archive (EXE, MSI, 7z, …)
/// that Lodge cannot extract directly. Resolution order:
///   1. winget  — built into Windows 10 1709+; handles almost everything
///   2. scoop   — if the user has Scoop CLI on PATH
///   3. error   — tells the user which tool to install
fn try_delegate(id: &str, version: &str, runtime_version: &str) -> Result<Receipt> {
    // ── winget ────────────────────────────────────────────────────────────────
    if probe_cmd("winget", &["--version"]) {
        let ok = std::process::Command::new("winget")
            .args([
                "install", id,
                "--accept-package-agreements",
                "--accept-source-agreements",
            ])
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .map(|s| s.success())
            .unwrap_or(false);

        if ok {
            return build_receipt(id, version, vec![], &[], runtime_version);
        }
        // winget was found but install failed — give a targeted message.
        bail!(
            "winget install {id} did not complete successfully. \
             run  winget install {id}  in a terminal to see what went wrong."
        );
    }

    // ── Scoop CLI ─────────────────────────────────────────────────────────────
    if probe_cmd("scoop", &["--version"]) {
        let ok = std::process::Command::new("scoop")
            .args(["install", id])
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .map(|s| s.success())
            .unwrap_or(false);

        if ok {
            return build_receipt(id, version, vec![], &[], runtime_version);
        }
        bail!(
            "scoop install {id} did not complete successfully. \
             run  scoop install {id}  in a terminal to see what went wrong."
        );
    }

    // ── neither available ─────────────────────────────────────────────────────
    bail!(
        "{id} uses a non-ZIP installer. \
         winget (built into Windows 10/11) or Scoop can handle it — \
         install either one and Lodge will use it automatically next time."
    )
}

/// Returns `true` if `cmd args` exits successfully (used to probe tool availability).
fn probe_cmd(cmd: &str, args: &[&str]) -> bool {
    std::process::Command::new(cmd)
        .args(args)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

/// Extract a human-readable author name from a homepage URL.
fn homepage_to_author(homepage: &str) -> String {
    // Strip protocol, take host, strip "www."
    let host = homepage
        .trim_start_matches("https://")
        .trim_start_matches("http://")
        .split('/')
        .next()
        .unwrap_or(homepage)
        .trim_start_matches("www.");
    host.to_string()
}

/// Convert a Unix epoch timestamp to (year, month, day, hour, min, sec).
fn epoch_to_parts(secs: u64) -> (u64, u64, u64, u64, u64, u64) {
    let sec   = secs % 60;
    let mins  = secs / 60;
    let min   = mins % 60;
    let hours = mins / 60;
    let hour  = hours % 24;
    let days  = hours / 24;

    // Gregorian calendar calculation
    let mut year = 1970u64;
    let mut remaining = days;
    loop {
        let leap = is_leap(year);
        let days_in_year = if leap { 366 } else { 365 };
        if remaining < days_in_year {
            break;
        }
        remaining -= days_in_year;
        year += 1;
    }
    let leap = is_leap(year);
    let month_days: &[u64] = if leap {
        &[31, 29, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31]
    } else {
        &[31, 28, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31]
    };
    let mut month = 1u64;
    for &md in month_days {
        if remaining < md {
            break;
        }
        remaining -= md;
        month += 1;
    }
    let day = remaining + 1;
    (year, month, day, hour, min, sec)
}

fn is_leap(year: u64) -> bool {
    year.is_multiple_of(400) || (year.is_multiple_of(4) && !year.is_multiple_of(100))
}
