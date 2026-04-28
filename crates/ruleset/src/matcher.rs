use std::path::Path;

use crate::types::Rule;

/// Returns the highest-priority rule matching `file_path` for `package_type`.
///
/// Matching strategy:
/// - Brace alternation (`{a,b,c}`) is expanded manually before matching, because
///   the `glob` crate does not support this syntax natively.
/// - For patterns **without** a `/`, the pattern is tested against both the full
///   relative path and the bare filename. This lets `*.exe` match `bin/setup.exe`.
/// - For patterns **with** a `/`, only the full relative path is tested, so
///   `bin/*` matches `bin/lodge` but not `lib/lodge`.
///
/// Lower `rule.priority` value wins. Returns `None` if no rule matches.
pub fn best_match<'a>(
    rules: &'a [Rule],
    package_type: &str,
    file_path: &str,
) -> Option<&'a Rule> {
    let filename = Path::new(file_path)
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or(file_path);

    let opts = glob::MatchOptions {
        case_sensitive: false,
        require_literal_separator: true,
        require_literal_leading_dot: false,
    };

    let mut candidates: Vec<&Rule> = rules
        .iter()
        .filter(|r| r.r#type == package_type)
        .filter(|r| {
            let patterns = expand_braces(&r.r#match);
            patterns.iter().any(|pat| {
                let Ok(compiled) = glob::Pattern::new(pat) else {
                    return false;
                };
                if r.r#match.contains('/') {
                    compiled.matches_with(file_path, opts)
                } else {
                    compiled.matches_with(file_path, opts)
                        || compiled.matches_with(filename, opts)
                }
            })
        })
        .collect();

    candidates.sort_by_key(|r| r.priority);
    candidates.into_iter().next()
}

/// Expands a single `{a,b,c}` brace group in a glob pattern into multiple patterns.
///
/// Only expands the first brace group found; for nested or multiple groups this
/// function is called recursively. If no braces are present the input is returned
/// as-is in a single-element `Vec`.
///
/// Example: `"*.{exe,dll}"` → `["*.exe", "*.dll"]`
fn expand_braces(pattern: &str) -> Vec<String> {
    let Some(open) = pattern.find('{') else {
        return vec![pattern.to_string()];
    };
    let Some(close_rel) = pattern[open..].find('}') else {
        return vec![pattern.to_string()]; // unmatched brace — pass through
    };
    let close = open + close_rel;

    let prefix = &pattern[..open];
    let suffix = &pattern[close + 1..];
    let alternatives = &pattern[open + 1..close];

    alternatives
        .split(',')
        .flat_map(|alt| expand_braces(&format!("{prefix}{}{suffix}", alt.trim())))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::loader::load_builtin;

    fn windows_rules() -> Vec<Rule> {
        load_builtin("windows").unwrap()
    }

    fn linux_rules() -> Vec<Rule> {
        load_builtin("linux").unwrap()
    }

    // ── Brace expansion ───────────────────────────────────────────────────────

    #[test]
    fn expand_braces_single() {
        let result = expand_braces("*.{exe,dll}");
        assert_eq!(result, vec!["*.exe", "*.dll"]);
    }

    #[test]
    fn expand_braces_triple() {
        let result = expand_braces("*.{json,yaml,toml}");
        assert_eq!(result, vec!["*.json", "*.yaml", "*.toml"]);
    }

    #[test]
    fn expand_braces_no_braces() {
        let result = expand_braces("*.exe");
        assert_eq!(result, vec!["*.exe"]);
    }

    #[test]
    fn expand_braces_with_prefix_and_suffix() {
        let result = expand_braces("lib/{foo,bar}.so");
        assert_eq!(result, vec!["lib/foo.so", "lib/bar.so"]);
    }

    // ── Basic matching ────────────────────────────────────────────────────────

    #[test]
    fn exe_matches_cli_tool() {
        let rules = windows_rules();
        let rule = best_match(&rules, "cli-tool", "lodge.exe").unwrap();
        assert_eq!(rule.id, "win-cli-exe");
    }

    #[test]
    fn exe_in_subdir_matches_by_filename() {
        let rules = windows_rules();
        let rule = best_match(&rules, "cli-tool", "bin/lodge.exe").unwrap();
        assert_eq!(rule.id, "win-cli-exe");
    }

    #[test]
    fn json_config_matches_cli_tool() {
        let rules = windows_rules();
        let rule = best_match(&rules, "cli-tool", "config.json");
        assert!(rule.is_some(), "json should match cli-tool config rule");
    }

    #[test]
    fn yaml_config_matches_cli_tool() {
        let rules = windows_rules();
        let rule = best_match(&rules, "cli-tool", "settings.yaml");
        assert!(rule.is_some(), "yaml should match cli-tool config rule");
    }

    #[test]
    fn font_ttf_matches_font_type() {
        let rules = windows_rules();
        let rule = best_match(&rules, "font", "roboto.ttf").unwrap();
        assert_eq!(rule.id, "win-font");
    }

    #[test]
    fn font_otf_matches_font_type() {
        let rules = windows_rules();
        let rule = best_match(&rules, "font", "roboto.otf").unwrap();
        assert_eq!(rule.id, "win-font");
    }

    // ── Path-sensitive patterns ───────────────────────────────────────────────

    #[test]
    fn bin_slash_pattern_matches_bin_dir() {
        let rules = linux_rules();
        let rule = best_match(&rules, "cli-tool", "bin/lodge");
        assert!(rule.is_some(), "bin/* should match bin/lodge");
    }

    #[test]
    fn bin_slash_pattern_does_not_match_lib_dir() {
        let rules = linux_rules();
        let rule = best_match(&rules, "cli-tool", "lib/helper.so");
        if let Some(r) = rule {
            assert_ne!(r.r#match, "bin/*", "bin/* must not match lib/ paths");
        }
    }

    // ── Priority ordering ─────────────────────────────────────────────────────

    #[test]
    fn lower_priority_wins_over_higher() {
        let rules = windows_rules();
        let rule = best_match(&rules, "cli-tool", "setup.exe").unwrap();
        assert!(rule.priority <= 20, "exe rule should win with low priority");
    }

    // ── Type filtering ────────────────────────────────────────────────────────

    #[test]
    fn cli_tool_rules_do_not_match_font_type() {
        let rules = windows_rules();
        let rule = best_match(&rules, "font", "setup.exe");
        assert!(rule.is_none(), "cli-tool exe rule must not apply to font package type");
    }

    #[test]
    fn no_match_returns_none() {
        let rules = windows_rules();
        let rule = best_match(&rules, "cli-tool", "readme.md");
        assert!(rule.is_none());
    }

    #[test]
    fn ps_module_rule_matches_psm1() {
        let rules = windows_rules();
        let rule = best_match(&rules, "ps-module", "MyModule.psm1").unwrap();
        assert_eq!(rule.id, "win-ps-module");
    }

    #[test]
    fn ps_module_rule_matches_ps1() {
        let rules = windows_rules();
        let rule = best_match(&rules, "ps-module", "helper.ps1").unwrap();
        assert_eq!(rule.id, "win-ps-module");
    }
}
