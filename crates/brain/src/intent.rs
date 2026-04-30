use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

/// The set of commands the intent resolver can return.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Command {
    Install,
    Uninstall,
    Update,
    UpdateAll,
    Search,
    List,
    Info,
    Verify,
    Rollback,
    UpdateRulesets,
    Use,
    History,
    Help,
    Explore,
    Clarify,
}

/// Structured output from the intent resolver.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Intent {
    pub command: Command,
    pub args: Value,
    pub confidence: f32,
    /// Set when `command == Clarify` — the question to ask the user.
    pub prompt: Option<String>,
}

fn intent(command: Command, args: Value, confidence: f32) -> Intent {
    Intent {
        command,
        args,
        confidence,
        prompt: None,
    }
}

fn clarify(prompt: impl Into<String>) -> Intent {
    Intent {
        command: Command::Clarify,
        args: json!({}),
        confidence: 0.3,
        prompt: Some(prompt.into()),
    }
}

/// Maps raw user input to a canonical [`Intent`] without a model.
///
/// Handles all known exact commands plus a set of explore patterns.
/// Used as the primary resolver when no model is loaded, and as the
/// fast-path fallback when the model returns low confidence.
pub fn resolve_deterministic(input: &str) -> Intent {
    let trimmed = input.trim();
    let lower = trimmed.to_lowercase();
    let words: Vec<&str> = lower.split_whitespace().collect();

    match words.as_slice() {
        ["help"] | ["?"] => intent(Command::Help, json!({}), 1.0),

        ["list"] | ["ls"] => intent(Command::List, json!({}), 1.0),
        ["what's", "installed"] | ["what", "is", "installed"] | ["installed"] => {
            intent(Command::List, json!({}), 0.95)
        }

        ["history"] => intent(Command::History, json!({}), 1.0),

        ["install", target, ..] => intent(Command::Install, json!({ "target": target }), 0.95),

        ["uninstall", id] | ["remove", id] => intent(Command::Uninstall, json!({ "id": id }), 0.95),

        ["update", "all"] => intent(Command::UpdateAll, json!({}), 1.0),
        ["update", "rulesets"] => intent(Command::UpdateRulesets, json!({}), 1.0),
        ["update", id] => intent(Command::Update, json!({ "id": id }), 0.95),

        ["rollback", id] => intent(Command::Rollback, json!({ "id": id }), 0.95),

        ["search", query, ..] => intent(Command::Search, json!({ "query": query }), 0.9),

        ["info", id] => intent(Command::Info, json!({ "id": id }), 0.95),

        ["verify", id] => intent(Command::Verify, json!({ "id": id }), 0.95),

        ["use", spec] => intent(Command::Use, json!({ "spec": spec }), 0.9),

        _ => classify_explore(&lower),
    }
}

/// Parses model-generated JSON into an [`Intent`], falling back to the
/// deterministic resolver on parse failure or low confidence.
///
/// Expected model output format:
/// ```json
/// { "command": "explore", "args": { "probe": "node_version", "probe_args": {} }, "confidence": 0.9 }
/// ```
pub fn resolve_from_json(json_str: &str, raw_input: &str) -> Intent {
    let Ok(value) = serde_json::from_str::<Value>(json_str) else {
        return resolve_deterministic(raw_input);
    };
    let Ok(intent) = serde_json::from_value::<Intent>(value) else {
        return resolve_deterministic(raw_input);
    };
    if intent.confidence < 0.6 {
        // Low confidence — prefer deterministic resolver if it's more confident
        let det = resolve_deterministic(raw_input);
        if det.confidence >= intent.confidence {
            return det;
        }
    }
    intent
}

// ── Explore classification ─────────────────────────────────────────────────

fn classify_explore(input: &str) -> Intent {
    let words: Vec<&str> = input.split_whitespace().collect();

    // Port probe
    if let Some(port) = extract_port(input) {
        return intent(
            Command::Explore,
            json!({ "probe": "port_in_use", "probe_args": { "port": port } }),
            0.85,
        );
    }

    // Node
    if words
        .iter()
        .any(|w| ["node", "nodejs", "node.js"].contains(w))
    {
        return intent(
            Command::Explore,
            json!({ "probe": "node_version", "probe_args": {} }),
            0.8,
        );
    }

    // PowerShell
    if words
        .iter()
        .any(|w| ["ps", "powershell", "pwsh"].contains(w))
        || input.contains("ps version")
        || input.contains("powershell version")
    {
        return intent(
            Command::Explore,
            json!({ "probe": "ps_version", "probe_args": {} }),
            0.85,
        );
    }

    // .NET
    if words.iter().any(|w| ["dotnet", ".net"].contains(w)) {
        return intent(
            Command::Explore,
            json!({ "probe": "dotnet_runtimes", "probe_args": {} }),
            0.85,
        );
    }

    // Python
    if words
        .iter()
        .any(|w| ["python", "python3", "py"].contains(w))
    {
        return intent(
            Command::Explore,
            json!({ "probe": "python_version", "probe_args": {} }),
            0.85,
        );
    }

    // Execution policy
    if input.contains("execution policy") || input.contains("executionpolicy") {
        return intent(
            Command::Explore,
            json!({ "probe": "execution_policy", "probe_args": {} }),
            0.9,
        );
    }

    // Disk / space / storage
    if words
        .iter()
        .any(|w| ["disk", "space", "free", "storage", "drive"].contains(w))
    {
        let path = extract_drive_or_path(input);
        return intent(
            Command::Explore,
            json!({ "probe": "disk_space", "probe_args": { "path": path } }),
            0.8,
        );
    }

    // OS / build
    if words
        .iter()
        .any(|w| ["os", "build", "winver", "uname", "version"].contains(w))
        && words
            .iter()
            .any(|w| ["os", "build", "windows", "linux", "mac"].contains(w))
    {
        return intent(
            Command::Explore,
            json!({ "probe": "os_build", "probe_args": {} }),
            0.75,
        );
    }

    // Architecture
    if words
        .iter()
        .any(|w| ["arch", "architecture", "cpu"].contains(w))
    {
        return intent(
            Command::Explore,
            json!({ "probe": "arch", "probe_args": {} }),
            0.85,
        );
    }

    // Service
    if words
        .iter()
        .any(|w| ["service", "daemon", "svc"].contains(w))
    {
        let name = extract_word_after(input, &["service", "daemon"]).unwrap_or_default();
        return intent(
            Command::Explore,
            json!({ "probe": "service_status", "probe_args": { "name": name } }),
            0.7,
        );
    }

    // Process
    if words
        .iter()
        .any(|w| ["process", "running", "processes", "task"].contains(w))
        && !words.contains(&"install")
    {
        let name = extract_word_after(input, &["process", "running"]).unwrap_or_default();
        return intent(
            Command::Explore,
            json!({ "probe": "process_running", "probe_args": { "name": name } }),
            0.7,
        );
    }

    // Env var
    if words
        .iter()
        .any(|w| ["env", "environment", "variable"].contains(w))
    {
        let name = extract_env_var_name(input);
        return intent(
            Command::Explore,
            json!({ "probe": "env_var", "probe_args": { "name": name } }),
            0.75,
        );
    }

    // Path / file / folder
    if words
        .iter()
        .any(|w| ["path", "exist", "exists", "file", "folder", "directory"].contains(w))
    {
        if let Some(path) = extract_quoted_or_backslash(input) {
            return intent(
                Command::Explore,
                json!({ "probe": "path_exists", "probe_args": { "path": path } }),
                0.75,
            );
        }
    }

    clarify("what would you like to know?")
}

// ── Extraction helpers ─────────────────────────────────────────────────────

fn extract_port(input: &str) -> Option<String> {
    // Match patterns like "port 8080", "port 3000 in use", "8080"
    let words: Vec<&str> = input.split_whitespace().collect();
    for (i, word) in words.iter().enumerate() {
        if *word == "port" {
            if let Some(next) = words.get(i + 1) {
                if next.chars().all(|c| c.is_ascii_digit()) {
                    return Some(next.to_string());
                }
            }
        }
        if word.chars().all(|c| c.is_ascii_digit()) {
            let n: u32 = word.parse().unwrap_or(0);
            if n > 0 && n <= 65535 {
                return Some(word.to_string());
            }
        }
    }
    None
}

fn extract_drive_or_path(input: &str) -> String {
    // Look for C:, D:, or a quoted path
    for word in input.split_whitespace() {
        if word.len() == 2
            && word.chars().next().unwrap_or(' ').is_alphabetic()
            && word.ends_with(':')
        {
            return format!("{word}\\");
        }
    }
    if let Some(p) = extract_quoted_or_backslash(input) {
        return p;
    }
    // Default to C:\ on Windows, / elsewhere
    if cfg!(windows) {
        "C:\\".into()
    } else {
        "/".into()
    }
}

fn extract_quoted_or_backslash(input: &str) -> Option<String> {
    // Extract "quoted" path
    if let Some(start) = input.find('"') {
        if let Some(end) = input[start + 1..].find('"') {
            return Some(input[start + 1..start + 1 + end].to_string());
        }
    }
    // Extract word containing backslash or forward slash
    for word in input.split_whitespace() {
        if word.contains('\\') || (word.contains('/') && word.len() > 1) {
            return Some(
                word.trim_matches(|c: char| {
                    !c.is_alphanumeric() && c != '\\' && c != '/' && c != ':'
                })
                .to_string(),
            );
        }
    }
    None
}

fn extract_word_after(input: &str, keywords: &[&str]) -> Option<String> {
    let words: Vec<&str> = input.split_whitespace().collect();
    for (i, word) in words.iter().enumerate() {
        if keywords.contains(word) {
            if let Some(next) = words.get(i + 1) {
                return Some(next.to_string());
            }
        }
    }
    None
}

fn extract_env_var_name(input: &str) -> String {
    // Look for %VAR% or $env:VAR or bare UPPERCASE_WORD
    if let Some(start) = input.find('%') {
        if let Some(end) = input[start + 1..].find('%') {
            return input[start + 1..start + 1 + end].to_string();
        }
    }
    if let Some(pos) = input.find("$env:") {
        let rest = &input[pos + 5..];
        return rest.split_whitespace().next().unwrap_or("").to_string();
    }
    // Look for an ALL_CAPS word
    for word in input.split_whitespace() {
        let clean: String = word
            .chars()
            .filter(|c| c.is_alphanumeric() || *c == '_')
            .collect();
        if clean.len() >= 2
            && clean
                .chars()
                .all(|c| c.is_uppercase() || c == '_' || c.is_ascii_digit())
        {
            return clean;
        }
    }
    String::new()
}

// ── System prompt ──────────────────────────────────────────────────────────

/// Generates the model system prompt from the probe dispatch table.
///
/// This ensures the prompt is always in sync with registered probes.
pub fn build_system_prompt() -> String {
    let probe_list: Vec<String> = crate::scout::PROBES
        .iter()
        .map(|p| {
            let args = if p.args.is_empty() {
                "no args".to_string()
            } else {
                p.args.join(", ")
            };
            format!("  - {} ({}): {}", p.name, args, p.description)
        })
        .collect();

    format!(
        r#"You are the intent resolver for Lodge, a package installation runtime.
You receive user input and return a JSON object. Never produce any output outside the JSON.

Known commands:
  install, uninstall, update, update-all, search, list, info, verify, rollback,
  update-rulesets, use, history, help, explore, clarify

For explore, include which probe to run and any required args:
{probes}

Output format:
  {{ "command": string, "args": object, "confidence": float, "prompt": string|null }}

For explore:
  {{ "command": "explore", "args": {{ "probe": "<name>", "probe_args": {{ ... }} }}, "confidence": float, "prompt": null }}

If confidence < 0.6, return:
  {{ "command": "clarify", "args": {{}}, "confidence": float, "prompt": "what did you mean?" }}

Examples:
  "install mytool" → {{ "command": "install", "args": {{ "target": "mytool" }}, "confidence": 0.98, "prompt": null }}
  "do I have node?" → {{ "command": "explore", "args": {{ "probe": "node_version", "probe_args": {{}} }}, "confidence": 0.95, "prompt": null }}
  "is port 8080 free?" → {{ "command": "explore", "args": {{ "probe": "port_in_use", "probe_args": {{ "port": "8080" }} }}, "confidence": 0.95, "prompt": null }}"#,
        probes = probe_list.join("\n")
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cmd(input: &str) -> Command {
        resolve_deterministic(input).command
    }

    #[test]
    fn exact_commands_resolve() {
        assert_eq!(cmd("help"), Command::Help);
        assert_eq!(cmd("list"), Command::List);
        assert_eq!(cmd("history"), Command::History);
        assert_eq!(cmd("update rulesets"), Command::UpdateRulesets);
        assert_eq!(cmd("update all"), Command::UpdateAll);
    }

    #[test]
    fn install_extracts_target() {
        let i = resolve_deterministic("install mytool");
        assert_eq!(i.command, Command::Install);
        assert_eq!(i.args["target"], "mytool");
        assert!(i.confidence >= 0.9);
    }

    #[test]
    fn update_extracts_id() {
        let i = resolve_deterministic("update mytool");
        assert_eq!(i.command, Command::Update);
        assert_eq!(i.args["id"], "mytool");
    }

    #[test]
    fn node_question_routes_to_explore() {
        let i = resolve_deterministic("do I have node installed");
        assert_eq!(i.command, Command::Explore);
        assert_eq!(i.args["probe"], "node_version");
    }

    #[test]
    fn port_pattern_extracts_number() {
        let i = resolve_deterministic("is port 3000 free");
        assert_eq!(i.command, Command::Explore);
        assert_eq!(i.args["probe"], "port_in_use");
        assert_eq!(i.args["probe_args"]["port"], "3000");
    }

    #[test]
    fn execution_policy_routes_correctly() {
        let i = resolve_deterministic("what is my execution policy");
        assert_eq!(i.command, Command::Explore);
        assert_eq!(i.args["probe"], "execution_policy");
    }

    #[test]
    fn unknown_input_clarifies() {
        let i = resolve_deterministic("xyzzy frobnicator");
        assert_eq!(i.command, Command::Clarify);
    }

    #[test]
    fn bad_json_falls_back_to_deterministic() {
        let i = resolve_from_json("not json at all", "help");
        assert_eq!(i.command, Command::Help);
    }

    #[test]
    fn low_confidence_json_falls_back() {
        let json = r#"{"command":"clarify","args":{},"confidence":0.2,"prompt":"huh?"}"#;
        let i = resolve_from_json(json, "list");
        // deterministic resolver should win for "list" which has confidence 1.0
        assert_eq!(i.command, Command::List);
    }

    #[test]
    fn system_prompt_contains_all_probes() {
        let prompt = build_system_prompt();
        for probe in crate::scout::PROBES {
            assert!(
                prompt.contains(probe.name),
                "probe {} missing from system prompt",
                probe.name
            );
        }
    }
}
