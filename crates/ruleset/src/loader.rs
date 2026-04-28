use anyhow::Result;

use crate::types::Rule;

/// Loads the built-in placement rules for `os`.
///
/// Valid values: `"windows"`, `"macos"`, `"linux"`.
/// Rules are embedded at compile time via [`include_str!`] — no file I/O at runtime.
pub fn load_builtin(os: &str) -> Result<Vec<Rule>> {
    let json = match os {
        "windows" => include_str!("../rules/windows.json"),
        "macos" => include_str!("../rules/macos.json"),
        "linux" => include_str!("../rules/linux.json"),
        other => anyhow::bail!("no built-in ruleset for os: {:?}", other),
    };
    let rules: Vec<Rule> =
        serde_json::from_str(json).map_err(|e| anyhow::anyhow!("malformed built-in ruleset for {os}: {e}"))?;
    Ok(rules)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn windows_ruleset_loads() {
        let rules = load_builtin("windows").unwrap();
        assert!(!rules.is_empty(), "windows ruleset must have at least one rule");
    }

    #[test]
    fn macos_ruleset_loads() {
        let rules = load_builtin("macos").unwrap();
        assert!(!rules.is_empty(), "macos ruleset must have at least one rule");
    }

    #[test]
    fn linux_ruleset_loads() {
        let rules = load_builtin("linux").unwrap();
        assert!(!rules.is_empty(), "linux ruleset must have at least one rule");
    }

    #[test]
    fn windows_has_cli_exe_rule() {
        let rules = load_builtin("windows").unwrap();
        assert!(
            rules.iter().any(|r| r.id == "win-cli-exe"),
            "windows ruleset must include win-cli-exe"
        );
    }

    #[test]
    fn linux_has_cli_bin_rule() {
        let rules = load_builtin("linux").unwrap();
        assert!(
            rules.iter().any(|r| r.id == "linux-cli-bin"),
            "linux ruleset must include linux-cli-bin"
        );
    }

    #[test]
    fn unknown_os_is_rejected() {
        assert!(load_builtin("haiku").is_err());
    }

    #[test]
    fn all_rules_have_valid_priorities() {
        for os in ["windows", "macos", "linux"] {
            let rules = load_builtin(os).unwrap();
            for rule in &rules {
                assert!(rule.priority > 0, "rule {} has zero priority", rule.id);
            }
        }
    }

    #[test]
    fn all_rules_have_non_empty_destinations() {
        for os in ["windows", "macos", "linux"] {
            let rules = load_builtin(os).unwrap();
            for rule in &rules {
                assert!(!rule.destination.user.is_empty(), "rule {} user dest is empty", rule.id);
                assert!(!rule.destination.system.is_empty(), "rule {} system dest is empty", rule.id);
            }
        }
    }
}
