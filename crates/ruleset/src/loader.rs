use anyhow::Result;
use crate::types::Rule;

/// Loads built-in rules for the given OS identifier (`"windows"`, `"macos"`, `"linux"`).
pub fn load_builtin(os: &str) -> Result<Vec<Rule>> {
    let json = match os {
        "windows" => include_str!("../rules/windows.json"),
        "macos"   => include_str!("../rules/macos.json"),
        "linux"   => include_str!("../rules/linux.json"),
        other     => anyhow::bail!("no built-in ruleset for os: {}", other),
    };
    let rules: Vec<Rule> = serde_json::from_str(json)?;
    Ok(rules)
}
