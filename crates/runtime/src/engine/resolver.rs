use std::path::Path;

use anyhow::Result;
use lodge_ruleset::{loader, matcher};
use lodge_shared::{
    manifest::{Hooks, Manifest, Scope},
    placement::{PlacementEntry, PlacementPlan, RegistrationEffects},
};

use super::{expander, inference::infer_scope};

/// Returns the current OS identifier used to select a ruleset.
pub fn current_os() -> &'static str {
    match std::env::consts::OS {
        "windows" => "windows",
        "macos" => "macos",
        _ => "linux",
    }
}

/// Resolves a [`PlacementPlan`] for all files under `package_root`.
///
/// Algorithm (M2 — no override handling yet, added in M3):
/// 1. Determine scope via [`infer_scope`].
/// 2. Walk `package_root` recursively.
/// 3. For each file: find best matching rule → use catch-all if none.
/// 4. Expand destination paths.
/// 5. Union registration effects across all matched rules.
/// 6. Collect lifecycle hooks in install order.
/// 7. Return [`PlacementPlan`].
pub fn resolve(
    package_root: &Path,
    manifest: &Manifest,
    os: &str,
    has_elevation: bool,
) -> Result<PlacementPlan> {
    let scope_res = infer_scope(manifest, has_elevation)?;
    if scope_res.fell_back {
        // Caller is responsible for surfacing this warning in the TUI.
        // We log it here as a debug hint.
        eprintln!(
            "note: {} preferred system scope but elevation is unavailable — \
             falling back to user scope",
            manifest.id
        );
    }

    let rules = loader::load_builtin(os)?;
    let package_type = package_type_str(&manifest.package_type);

    let mut entries: Vec<PlacementEntry> = Vec::new();
    let mut registrations = RegistrationEffects::default();

    for entry in walkdir::WalkDir::new(package_root)
        .min_depth(1)
        .into_iter()
        .filter_map(|e| e.ok())
        .filter(|e| e.file_type().is_file())
    {
        let abs_source = entry.path().to_path_buf();
        let rel_path = abs_source
            .strip_prefix(package_root)
            .unwrap_or(&abs_source)
            .to_string_lossy()
            .replace('\\', "/"); // normalise to forward slashes for matching

        let (dest_template, rule_registrations) = match matcher::best_match(&rules, package_type, &rel_path) {
            Some(rule) => {
                let template = dest_template_for_scope(rule, &scope_res.scope);
                let reg = rule_to_registrations(rule, manifest);
                (template.to_string(), reg)
            }
            None => {
                let template = catch_all(os, &scope_res.scope).to_string();
                (template, RegistrationEffects::default())
            }
        };

        merge_registrations(&mut registrations, rule_registrations);

        let dest_dir = expander::expand(&dest_template, &manifest.id)?;
        let file_name = abs_source
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("unknown");
        let destination = dest_dir.join(file_name);

        entries.push(PlacementEntry {
            source: abs_source,
            destination,
            rename: None,
        });
    }

    let hooks_order = collect_install_hooks(&manifest.hooks);

    Ok(PlacementPlan {
        entries,
        registrations,
        hooks_order,
        requires_elevation: matches!(scope_res.scope, Scope::System),
    })
}

// ── Internal helpers ──────────────────────────────────────────────────────────

fn package_type_str(pt: &lodge_shared::manifest::PackageType) -> &'static str {
    use lodge_shared::manifest::PackageType::*;
    match pt {
        CliTool => "cli-tool",
        PsModule => "ps-module",
        Service => "service",
        Library => "library",
        App => "app",
        ConfigPack => "config-pack",
        DevTool => "dev-tool",
        Font => "font",
    }
}

fn dest_template_for_scope<'a>(rule: &'a lodge_ruleset::types::Rule, scope: &Scope) -> &'a str {
    match scope {
        Scope::User => &rule.destination.user,
        Scope::System => &rule.destination.system,
    }
}

fn rule_to_registrations(
    rule: &lodge_ruleset::types::Rule,
    manifest: &Manifest,
) -> RegistrationEffects {
    RegistrationEffects {
        add_to_path: rule.register.path,
        env_var: if rule.register.env_var {
            manifest.naming.env_var.clone()
        } else {
            None
        },
        service_name: if rule.register.service {
            manifest.naming.service.clone()
        } else {
            None
        },
        start_menu_entry: rule.register.start_menu,
    }
}

fn merge_registrations(base: &mut RegistrationEffects, other: RegistrationEffects) {
    base.add_to_path |= other.add_to_path;
    if base.env_var.is_none() {
        base.env_var = other.env_var;
    }
    if base.service_name.is_none() {
        base.service_name = other.service_name;
    }
    base.start_menu_entry |= other.start_menu_entry;
}

/// Catch-all destination when no rule matches a file.
fn catch_all(os: &str, scope: &Scope) -> &'static str {
    match (os, scope) {
        ("windows", Scope::User) => "%APPDATA%\\{id}\\",
        ("windows", Scope::System) => "%ProgramData%\\{id}\\",
        (_, Scope::User) => "~/.local/share/{id}/",
        (_, Scope::System) => "/usr/local/share/{id}/",
    }
}

/// Returns the ordered list of install-time lifecycle hooks to run.
fn collect_install_hooks(hooks: &Hooks) -> Vec<String> {
    let mut order = Vec::new();
    if let Some(h) = &hooks.pre_install {
        order.push(h.clone());
    }
    if let Some(h) = &hooks.post_install {
        order.push(h.clone());
    }
    order
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use lodge_shared::manifest::{Manifest, PackageType, Prefers, Scope};
    use std::fs;

    fn cli_manifest(id: &str) -> Manifest {
        Manifest {
            id: id.to_string(),
            version: "1.0.0".into(),
            package_type: PackageType::CliTool,
            description: None,
            author: None,
            prefers: Prefers { scope: Some(Scope::User), ..Default::default() },
            requires: Default::default(),
            naming: Default::default(),
            overrides: vec![],
            hooks: Default::default(),
        }
    }

    /// Creates a temp package directory with the given relative file paths.
    fn make_package(files: &[&str]) -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        for rel in files {
            let full = dir.path().join(rel);
            if let Some(parent) = full.parent() {
                fs::create_dir_all(parent).unwrap();
            }
            fs::write(&full, b"placeholder").unwrap();
        }
        dir
    }

    // ── Hooks collection ──────────────────────────────────────────────────────

    #[test]
    fn hooks_collected_in_install_order() {
        use lodge_shared::manifest::Hooks;
        let hooks = Hooks {
            pre_install: Some("pre.ps1".into()),
            post_install: Some("post.ps1".into()),
            pre_uninstall: Some("pre-un.ps1".into()),
            post_uninstall: Some("post-un.ps1".into()),
        };
        let order = collect_install_hooks(&hooks);
        assert_eq!(order, vec!["pre.ps1", "post.ps1"]);
        // uninstall hooks must NOT appear in install order
        assert!(!order.iter().any(|h| h.contains("uninstall")));
    }

    #[test]
    fn no_hooks_returns_empty_order() {
        let order = collect_install_hooks(&Default::default());
        assert!(order.is_empty());
    }

    // ── catch_all ─────────────────────────────────────────────────────────────

    #[test]
    fn catch_all_windows_user() {
        assert!(catch_all("windows", &Scope::User).contains("%APPDATA%"));
    }

    #[test]
    fn catch_all_linux_user() {
        assert!(catch_all("linux", &Scope::User).contains("~/.local/share"));
    }

    // ── package_type_str ─────────────────────────────────────────────────────

    #[test]
    fn all_package_types_have_string_repr() {
        use PackageType::*;
        for pt in [CliTool, PsModule, Service, Library, App, ConfigPack, DevTool, Font] {
            let s = package_type_str(&pt);
            assert!(!s.is_empty());
        }
    }

    // ── Full resolve on Linux (HOME is set by the OS in test env) ─────────────

    #[test]
    fn resolve_places_bin_file_for_cli_tool() {
        // This test requires HOME to be set (always true on Linux/macOS CI).
        // On Windows it uses USERPROFILE which is always set.
        if std::env::var("HOME").is_err() && std::env::var("USERPROFILE").is_err() {
            return; // skip if neither is available
        }

        let pkg = make_package(&["bin/lodge"]);
        let manifest = cli_manifest("lodge");

        let plan = resolve(pkg.path(), &manifest, "linux", false).unwrap();
        assert_eq!(plan.entries.len(), 1);
        assert!(
            plan.entries[0].destination.to_string_lossy().contains("lodge"),
            "destination should contain the package id"
        );
        assert!(!plan.requires_elevation, "user scope must not require elevation");
    }

    #[test]
    fn resolve_returns_empty_plan_for_empty_package() {
        let pkg = make_package(&[]);
        let manifest = cli_manifest("empty-pkg");
        let plan = resolve(pkg.path(), &manifest, "linux", false).unwrap();
        assert!(plan.entries.is_empty());
        assert!(plan.hooks_order.is_empty());
    }

    #[test]
    fn resolve_uses_catch_all_for_unrecognised_files() {
        if std::env::var("HOME").is_err() && std::env::var("USERPROFILE").is_err() {
            return;
        }
        let pkg = make_package(&["readme.md"]);
        let manifest = cli_manifest("mytool");
        let plan = resolve(pkg.path(), &manifest, "linux", false).unwrap();
        // readme.md has no cli-tool rule → catch-all → should still produce an entry
        assert_eq!(plan.entries.len(), 1);
        assert!(plan.entries[0].destination.to_string_lossy().contains("mytool"));
    }

    #[test]
    fn resolve_system_scope_requires_elevation_in_plan() {
        let pkg = make_package(&["bin/tool"]);
        let mut manifest = cli_manifest("tool");
        manifest.prefers.scope = Some(Scope::System);

        if std::env::var("HOME").is_err() && std::env::var("USERPROFILE").is_err() {
            return;
        }
        let plan = resolve(pkg.path(), &manifest, "linux", true).unwrap();
        assert!(plan.requires_elevation, "system scope must set requires_elevation");
    }
}
