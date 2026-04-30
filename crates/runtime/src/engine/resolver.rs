use std::path::Path;

use anyhow::Result;
use lodge_ruleset::{loader, matcher};
use lodge_shared::{
    manifest::{Hooks, Manifest, Override, Scope},
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
/// Algorithm:
/// 1. Determine scope via [`infer_scope`].
/// 2. Walk `package_root` recursively.
/// 3. For each file:
///    a. Check `manifest.overrides` for explicit match → use if found.
///    b. Check ruleset for type + file pattern match → use highest priority match.
///    c. If no rule matches → use catch-all destination.
/// 4. Expand all destination paths (env vars, `~`, `{id}`).
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

    for dir_entry in walkdir::WalkDir::new(package_root)
        .min_depth(1)
        .into_iter()
        .filter_map(|e| e.ok())
        .filter(|e| e.file_type().is_file())
    {
        let abs_source = dir_entry.path().to_path_buf();
        let rel_path = abs_source
            .strip_prefix(package_root)
            .unwrap_or(&abs_source)
            .to_string_lossy()
            .replace('\\', "/");

        // Step 3a — explicit override takes priority over all rules.
        if let Some(ov) = find_override(&manifest.overrides, &rel_path) {
            let dest_dir = expander::expand(&ov.destination, &manifest.id)?;
            let file_name = ov
                .rename
                .as_deref()
                .or_else(|| abs_source.file_name().and_then(|n| n.to_str()))
                .unwrap_or("unknown");
            let destination = dest_dir.join(file_name);
            entries.push(PlacementEntry {
                source: abs_source,
                destination,
                rename: ov.rename.clone(),
            });
            continue;
        }

        // Step 3b — ruleset match.
        let (dest_template, rule_registrations) =
            match matcher::best_match(&rules, package_type, &rel_path) {
                Some(rule) => {
                    let template = dest_template_for_scope(rule, &scope_res.scope);
                    let reg = rule_to_registrations(rule, manifest);
                    (template.to_string(), reg)
                }
                // Step 3c — catch-all.
                None => (catch_all(os, &scope_res.scope).to_string(), RegistrationEffects::default()),
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

/// Finds the first override whose glob pattern matches `rel_path`.
fn find_override<'a>(overrides: &'a [Override], rel_path: &str) -> Option<&'a Override> {
    let filename = std::path::Path::new(rel_path)
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or(rel_path);

    overrides.iter().find(|ov| {
        let Ok(pat) = glob::Pattern::new(&ov.pattern) else {
            return false;
        };
        let opts = glob::MatchOptions {
            case_sensitive: false,
            require_literal_separator: true,
            require_literal_leading_dot: false,
        };
        pat.matches_with(rel_path, opts) || pat.matches_with(filename, opts)
    })
}

pub fn package_type_str(pt: &lodge_shared::manifest::PackageType) -> &'static str {
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
        env_var: if rule.register.env_var { manifest.naming.env_var.clone() } else { None },
        service_name: if rule.register.service { manifest.naming.service.clone() } else { None },
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

fn catch_all(os: &str, scope: &Scope) -> &'static str {
    match (os, scope) {
        ("windows", Scope::User) => "%APPDATA%\\{id}\\",
        ("windows", Scope::System) => "%ProgramData%\\{id}\\",
        (_, Scope::User) => "~/.local/share/{id}/",
        (_, Scope::System) => "/usr/local/share/{id}/",
    }
}

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
    use lodge_shared::manifest::{Manifest, Override, PackageType, Prefers, Scope};
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

    // ── Override handling ─────────────────────────────────────────────────────

    #[test]
    fn find_override_matches_glob() {
        let overrides = vec![Override {
            pattern: "*.cfg".into(),
            destination: "/tmp/config/".into(),
            rename: None,
        }];
        assert!(find_override(&overrides, "app.cfg").is_some());
        assert!(find_override(&overrides, "app.exe").is_none());
    }

    #[test]
    fn find_override_matches_by_filename() {
        let overrides = vec![Override {
            pattern: "*.cfg".into(),
            destination: "/tmp/config/".into(),
            rename: None,
        }];
        assert!(find_override(&overrides, "sub/app.cfg").is_some());
    }

    #[test]
    fn override_takes_priority_over_rules() {
        if std::env::var("HOME").is_err() && std::env::var("USERPROFILE").is_err() {
            return;
        }
        let pkg = make_package(&["tool.exe"]);
        let mut manifest = cli_manifest("tool");
        manifest.overrides.push(Override {
            pattern: "*.exe".into(),
            destination: "~/.custom/".into(),
            rename: None,
        });

        let plan = resolve(pkg.path(), &manifest, "linux", false).unwrap();
        assert_eq!(plan.entries.len(), 1);
        assert!(
            plan.entries[0].destination.to_string_lossy().contains(".custom"),
            "override destination should be used"
        );
    }

    #[test]
    fn override_rename_applies() {
        if std::env::var("HOME").is_err() && std::env::var("USERPROFILE").is_err() {
            return;
        }
        let pkg = make_package(&["mt.exe"]);
        let mut manifest = cli_manifest("mytool");
        manifest.overrides.push(Override {
            pattern: "*.exe".into(),
            destination: "~/.custom/".into(),
            rename: Some("mytool".into()),
        });

        let plan = resolve(pkg.path(), &manifest, "linux", false).unwrap();
        assert_eq!(plan.entries[0].destination.file_name().unwrap(), "mytool");
    }

    // ── Resolver (inherited from M2) ──────────────────────────────────────────

    #[test]
    fn hooks_collected_in_install_order() {
        use lodge_shared::manifest::Hooks;
        let hooks = Hooks {
            pre_install: Some("pre.ps1".into()),
            post_install: Some("post.ps1".into()),
            ..Default::default()
        };
        let order = collect_install_hooks(&hooks);
        assert_eq!(order, vec!["pre.ps1", "post.ps1"]);
    }

    #[test]
    fn resolve_places_bin_file_for_cli_tool() {
        if std::env::var("HOME").is_err() && std::env::var("USERPROFILE").is_err() {
            return;
        }
        let pkg = make_package(&["bin/lodge"]);
        let plan = resolve(pkg.path(), &cli_manifest("lodge"), "linux", false).unwrap();
        assert_eq!(plan.entries.len(), 1);
    }

    #[test]
    fn resolve_returns_empty_plan_for_empty_package() {
        let pkg = make_package(&[]);
        let plan = resolve(pkg.path(), &cli_manifest("empty"), "linux", false).unwrap();
        assert!(plan.entries.is_empty());
    }

    #[test]
    fn all_package_types_have_string_repr() {
        use PackageType::*;
        for pt in [CliTool, PsModule, Service, Library, App, ConfigPack, DevTool, Font] {
            assert!(!package_type_str(&pt).is_empty());
        }
    }
}
