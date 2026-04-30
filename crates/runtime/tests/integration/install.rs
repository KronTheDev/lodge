/// Integration tests for the full resolve → execute → receipt pipeline.
///
/// Each test uses a fixture package from `tests/fixtures/packages/`.
/// Files are placed into a `tempfile::tempdir()` so nothing touches the real system.
use std::path::Path;

use lodge::engine::{attester, executor, manifest, resolver};
use lodge_shared::manifest::Scope;
use tempfile::tempdir;

fn fixtures() -> &'static Path {
    // CARGO_MANIFEST_DIR = crates/runtime — step up to workspace root
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap() // crates/
        .parent()
        .unwrap() // workspace root
        .join("tests/fixtures/packages")
        .leak()
}

// ── minimal ───────────────────────────────────────────────────────────────────

#[test]
fn minimal_manifest_parses() {
    let pkg = fixtures().join("minimal");
    let json = std::fs::read_to_string(pkg.join("lodge.json")).unwrap();
    let m = manifest::parse(&json).unwrap();
    assert_eq!(m.id, "minimal");
    assert_eq!(m.version, "1.0.0");
}

#[test]
fn minimal_resolves_without_error() {
    let pkg = fixtures().join("minimal");
    let json = std::fs::read_to_string(pkg.join("lodge.json")).unwrap();
    let m = manifest::parse(&json).unwrap();
    let os = resolver::current_os();
    let plan = resolver::resolve(&pkg, &m, os, false).unwrap();
    // minimal package has one .exe file — should produce at least one entry
    assert!(
        !plan.entries.is_empty(),
        "expected at least one placement entry"
    );
}

#[test]
fn minimal_execute_places_files_in_temp() {
    let pkg = fixtures().join("minimal");
    let json = std::fs::read_to_string(pkg.join("lodge.json")).unwrap();
    let m = manifest::parse(&json).unwrap();

    // Resolve against Windows OS so we get a predictable destination template
    let plan = resolver::resolve(&pkg, &m, "windows", false).unwrap();

    // Redirect all destinations into a temp dir
    let dest_root = tempdir().unwrap();
    let redirected = redirect_plan_to(plan, dest_root.path());

    let mut events = Vec::new();
    executor::execute(&redirected, &pkg, &mut |e| events.push(e)).unwrap();

    // Every entry should have ended up Done
    use lodge::engine::executor::StepState;
    let failed: Vec<_> = events
        .iter()
        .filter(|e| matches!(e.state, StepState::Failed(_)))
        .collect();
    assert!(failed.is_empty(), "unexpected failures: {failed:?}");
}

#[test]
fn minimal_receipt_written_and_verifiable() {
    let pkg = fixtures().join("minimal");
    let json = std::fs::read_to_string(pkg.join("lodge.json")).unwrap();
    let m = manifest::parse(&json).unwrap();
    let plan = resolver::resolve(&pkg, &m, "windows", false).unwrap();

    let dest_root = tempdir().unwrap();
    let redirected = redirect_plan_to(plan, dest_root.path());

    executor::execute(&redirected, &pkg, &mut |_| {}).unwrap();

    // Override receipt dir to tempdir via env
    let receipt_root = tempdir().unwrap();
    unsafe {
        std::env::set_var("LOCALAPPDATA", receipt_root.path());
    }
    let receipt = attester::write_receipt(&m, &redirected, &Scope::User, vec![], "0.1.0").unwrap();
    unsafe {
        std::env::remove_var("LOCALAPPDATA");
    }

    assert_eq!(receipt.id, "minimal");
    assert!(!receipt.receipt_hash.is_empty());
    assert!(attester::verify_receipt(&receipt));
}

// ── cli-full ──────────────────────────────────────────────────────────────────

#[test]
fn cli_full_manifest_parses_all_fields() {
    let pkg = fixtures().join("cli-full");
    let json = std::fs::read_to_string(pkg.join("lodge.json")).unwrap();
    let m = manifest::parse(&json).unwrap();
    assert_eq!(m.id, "cli-full");
    assert_eq!(m.version, "2.3.1");
    assert_eq!(m.command_name(), "cft");
    assert_eq!(m.naming.env_var.as_deref(), Some("CLI_FULL_HOME"));
}

#[test]
fn cli_full_resolves_multiple_entries() {
    let pkg = fixtures().join("cli-full");
    let json = std::fs::read_to_string(pkg.join("lodge.json")).unwrap();
    let m = manifest::parse(&json).unwrap();
    let plan = resolver::resolve(&pkg, &m, "windows", false).unwrap();
    // .exe and .json should both produce entries
    assert!(
        plan.entries.len() >= 2,
        "expected at least 2 entries, got {}",
        plan.entries.len()
    );
}

// ── with-overrides ────────────────────────────────────────────────────────────

#[test]
fn overrides_rename_applied() {
    let pkg = fixtures().join("with-overrides");
    let json = std::fs::read_to_string(pkg.join("lodge.json")).unwrap();
    let m = manifest::parse(&json).unwrap();
    let plan = resolver::resolve(&pkg, &m, "windows", false).unwrap();

    // The override renames with-overrides.exe → wo.exe
    let exe_entry = plan
        .entries
        .iter()
        .find(|e| e.source.file_name().and_then(|n| n.to_str()) == Some("with-overrides.exe"));

    assert!(
        exe_entry.is_some(),
        "couldn't find with-overrides.exe entry"
    );
    let entry = exe_entry.unwrap();
    assert_eq!(
        entry.rename.as_deref(),
        Some("wo.exe"),
        "override rename not applied"
    );
}

#[test]
fn overrides_take_priority_over_rules() {
    let pkg = fixtures().join("with-overrides");
    let json = std::fs::read_to_string(pkg.join("lodge.json")).unwrap();
    let m = manifest::parse(&json).unwrap();
    let plan = resolver::resolve(&pkg, &m, "windows", false).unwrap();

    // The .dll override routes to %APPDATA%\with-overrides\lib\
    let dll_entry = plan
        .entries
        .iter()
        .find(|e| e.source.extension().and_then(|x| x.to_str()) == Some("dll"));

    assert!(dll_entry.is_some(), "couldn't find .dll entry");
    let dest = dll_entry
        .unwrap()
        .destination
        .to_string_lossy()
        .to_lowercase();
    assert!(
        dest.contains("with-overrides"),
        "dll destination should contain package id, got: {dest}"
    );
}

// ── Helpers ───────────────────────────────────────────────────────────────────

/// Redirect all plan entry destinations into `root` so tests don't touch real system paths.
fn redirect_plan_to(
    mut plan: lodge_shared::placement::PlacementPlan,
    root: &Path,
) -> lodge_shared::placement::PlacementPlan {
    for entry in &mut plan.entries {
        let file_name = entry.source.file_name().unwrap_or_default().to_os_string();
        entry.destination = root.join(&file_name);
    }
    plan
}
