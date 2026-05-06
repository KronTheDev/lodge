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
    /// Expand the last probe result using the configured AI provider.
    Expand,
    /// Run the full probe battery and narrate the result.
    Scan,
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
        // ── Expand — AI deeper explanation of last probe result ─────────────
        ["expand"] => resolve_expand(&[]),
        ["expand", rest @ ..] => resolve_expand(rest),

        // ── Scan — full probe battery ────────────────────────────────────────
        ["scan"] | ["scan", "system"] | ["scan", "my", "system"]
        | ["check", "my", "system"] | ["full", "scan"] | ["system", "scan"] => {
            intent(Command::Scan, json!({}), 1.0)
        }

        // ── Help ────────────────────────────────────────────────────────────
        ["help"] | ["?"] | ["commands"] | ["what", "can", "you", "do"]
        | ["what", "can", "i", "do"] => intent(Command::Help, json!({}), 1.0),

        // ── List ────────────────────────────────────────────────────────────
        ["list"] | ["ls"] => intent(Command::List, json!({}), 1.0),
        ["what's", "installed"]
        | ["what", "is", "installed"]
        | ["installed"]
        | ["show", "installed"]
        | ["what", "do", "i", "have"]
        | ["what's", "here"]
        | ["show", "packages"]
        | ["packages"] => intent(Command::List, json!({}), 0.95),

        // ── History ─────────────────────────────────────────────────────────
        ["history"] | ["log"] | ["past", "installs"] | ["install", "history"] => {
            intent(Command::History, json!({}), 1.0)
        }

        // ── Uninstall (specific multi-word forms first) ──────────────────────
        ["get", "rid", "of", id] | ["take", "out", id] => {
            intent(Command::Uninstall, json!({ "id": id }), 0.95)
        }
        ["uninstall", id] | ["remove", id] | ["delete", id] | ["trash", id] | ["drop", id] => {
            intent(Command::Uninstall, json!({ "id": id }), 0.95)
        }

        // ── Install ─────────────────────────────────────────────────────────
        // "install" is always explicit — trust it unconditionally.
        ["install", target, ..] => {
            intent(Command::Install, json!({ "target": target }), 0.95)
        }
        // Synonyms (get, grab, …) only fire when the first argument looks like
        // a package ID, not a pronoun or article ("get me my OS specs" should
        // fall through to classify_explore).
        ["set", "up", target, ..] if looks_like_id(target) => {
            intent(Command::Install, json!({ "target": target }), 0.95)
        }
        ["get", target, ..]
        | ["grab", target, ..]
        | ["fetch", target, ..]
        | ["add", target, ..]
        | ["setup", target, ..] if looks_like_id(target) => {
            intent(Command::Install, json!({ "target": target }), 0.95)
        }

        // ── Update ───────────────────────────────────────────────────────────
        ["update", "all"] | ["upgrade", "all"] | ["update", "everything"] => {
            intent(Command::UpdateAll, json!({}), 1.0)
        }
        ["update", "rulesets"] | ["refresh", "rulesets"] => {
            intent(Command::UpdateRulesets, json!({}), 1.0)
        }
        ["update", id] | ["upgrade", id] | ["refresh", id] => {
            intent(Command::Update, json!({ "id": id }), 0.95)
        }

        // ── Rollback ─────────────────────────────────────────────────────────
        ["go", "back", id] => intent(Command::Rollback, json!({ "id": id }), 0.95),
        ["rollback", id]
        | ["revert", id]
        | ["undo", id]
        | ["downgrade", id]
        | ["previous", id] => intent(Command::Rollback, json!({ "id": id }), 0.95),

        // ── Search ───────────────────────────────────────────────────────────
        ["search"] | ["browse"] => intent(Command::Search, json!({ "query": "" }), 1.0),
        ["look", "for", query, ..] => intent(Command::Search, json!({ "query": query }), 0.9),
        ["search", query, ..]
        | ["find", query, ..]
        | ["lookup", query, ..] => intent(Command::Search, json!({ "query": query }), 0.9),

        // ── Info ─────────────────────────────────────────────────────────────
        ["tell", "me", "about", id] | ["show", "me", id] => {
            intent(Command::Info, json!({ "id": id }), 0.9)
        }
        ["what", "about", id] => intent(Command::Info, json!({ "id": id }), 0.8),
        ["info", id]
        | ["about", id]
        | ["show", id]
        | ["details", id]
        | ["what", "is", id]
        | ["describe", id] => intent(Command::Info, json!({ "id": id }), 0.95),

        // ── Verify ───────────────────────────────────────────────────────────
        ["verify", id]
        | ["check", id]
        | ["validate", id]
        | ["integrity", id]
        | ["check", "integrity", id] => intent(Command::Verify, json!({ "id": id }), 0.95),

        // ── Use ──────────────────────────────────────────────────────────────
        ["use", spec]
        | ["switch", "to", spec]
        | ["switch", spec]
        | ["activate", spec]
        | ["pin", spec] => intent(Command::Use, json!({ "spec": spec }), 0.9),

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
    // Strip trailing `?` so "is node installed?" matches "is node installed".
    let input = input.trim_end_matches('?').trim();

    // Strip leading/trailing punctuation from each token, but preserve `%` and `$`
    // so env var patterns like `%APPDATA%` and `$HOME` survive intact.
    let stripped: Vec<String> = input
        .split_whitespace()
        .map(|w| w.trim_matches(|c: char| !c.is_alphanumeric() && c != '%' && c != '$').to_string())
        .collect();
    let words: Vec<&str> = stripped.iter().map(String::as_str).collect();

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

    // .NET — ".NET" loses its leading dot after punctuation stripping, so also
    // match the bare "net" token when the original input contains ".net".
    if words.contains(&"dotnet")
        || (input.contains(".net") && words.contains(&"net"))
    {
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

    // Git
    if words.contains(&"git") {
        return intent(
            Command::Explore,
            json!({ "probe": "git_version", "probe_args": {} }),
            0.85,
        );
    }

    // Java / JDK / JRE
    if words
        .iter()
        .any(|w| ["java", "jdk", "jre", "jvm"].contains(w))
    {
        return intent(
            Command::Explore,
            json!({ "probe": "java_version", "probe_args": {} }),
            0.85,
        );
    }

    // Go / Golang — "go" alone is too ambiguous; require "golang" or qualifier
    if words.iter().any(|w| ["golang"].contains(w))
        || (words.contains(&"go")
            && words
                .iter()
                .any(|w| ["version", "installed", "runtime", "compiler"].contains(w)))
    {
        return intent(
            Command::Explore,
            json!({ "probe": "go_version", "probe_args": {} }),
            0.85,
        );
    }

    // Ruby / Rails / Gem
    if words
        .iter()
        .any(|w| ["ruby", "rails", "gem", "bundler"].contains(w))
    {
        return intent(
            Command::Explore,
            json!({ "probe": "ruby_version", "probe_args": {} }),
            0.85,
        );
    }

    // Docker / containers
    if words
        .iter()
        .any(|w| ["docker", "container", "containers", "dockerfile"].contains(w))
    {
        return intent(
            Command::Explore,
            json!({ "probe": "docker_version", "probe_args": {} }),
            0.85,
        );
    }

    // npm
    if words.contains(&"npm") {
        return intent(
            Command::Explore,
            json!({ "probe": "npm_version", "probe_args": {} }),
            0.9,
        );
    }

    // PHP
    if words.contains(&"php") {
        return intent(
            Command::Explore,
            json!({ "probe": "php_version", "probe_args": {} }),
            0.9,
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

    // RAM / memory (check before disk to avoid "free" keyword collision)
    if words
        .iter()
        .any(|w| ["ram", "memory", "mem"].contains(w))
        && !words.iter().any(|w| ["disk", "storage"].contains(w))
    {
        return intent(
            Command::Explore,
            json!({ "probe": "ram_usage", "probe_args": {} }),
            0.85,
        );
    }

    // Multiple explicit drive letters in one query ("C: and D:", "C:/ and D:/")
    {
        let drive_count = input.split_whitespace()
            .filter(|w| {
                let s: String = w.chars().filter(|c| c.is_alphanumeric() || *c == ':').collect();
                s.len() == 2
                    && s.ends_with(':')
                    && s.chars().next().map(|c| c.is_alphabetic()).unwrap_or(false)
            })
            .count();
        if drive_count >= 2 {
            return intent(
                Command::Explore,
                json!({ "probe": "disk_space_all", "probe_args": {} }),
                0.85,
            );
        }
    }

    // Disk / space / storage — all drives
    if words
        .iter()
        .any(|w| ["disk", "space", "free", "storage", "drive", "drives"].contains(w))
        && words
            .iter()
            .any(|w| ["all", "total", "combined", "every"].contains(w))
    {
        return intent(
            Command::Explore,
            json!({ "probe": "disk_space_all", "probe_args": {} }),
            0.85,
        );
    }

    // Disk / space / storage — single drive
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

    // OS / build — "get me my OS specs", "what OS am I on", "system specs", etc.
    if words
        .iter()
        .any(|w| ["os", "build", "winver", "uname"].contains(w))
        || (words.iter().any(|w| ["specs", "spec", "specifications"].contains(w))
            && words.iter().any(|w| ["os", "system", "rig", "pc", "computer", "machine"].contains(w)))
        || (words.iter().any(|w| ["os", "operating", "system"].contains(w))
            && words.iter().any(|w| ["version", "build", "am", "my", "running"].contains(w)))
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

    // CPU / processor
    if words
        .iter()
        .any(|w| ["cpu", "processor", "cores", "threads", "chip"].contains(w))
    {
        return intent(
            Command::Explore,
            json!({ "probe": "cpu_info", "probe_args": {} }),
            0.85,
        );
    }

    // Uptime / last reboot
    if words
        .iter()
        .any(|w| ["uptime", "rebooted", "booted", "restarted", "restart"].contains(w))
        || (words.contains(&"long")
            && words.iter().any(|w| ["up", "running", "on"].contains(w)))
    {
        return intent(
            Command::Explore,
            json!({ "probe": "uptime", "probe_args": {} }),
            0.85,
        );
    }

    // Hostname / computer name
    if words
        .iter()
        .any(|w| ["hostname", "computername"].contains(w))
        || (words
            .iter()
            .any(|w| ["computer", "machine", "pc"].contains(w))
            && words.iter().any(|w| ["name", "called", "named"].contains(w)))
    {
        return intent(
            Command::Explore,
            json!({ "probe": "hostname", "probe_args": {} }),
            0.85,
        );
    }

    // Current user / username
    if words
        .iter()
        .any(|w| ["username", "whoami"].contains(w))
        || (words.contains(&"who") && words.contains(&"i"))
        || (words.iter().any(|w| ["current", "logged"].contains(w))
            && words.contains(&"user"))
    {
        return intent(
            Command::Explore,
            json!({ "probe": "username", "probe_args": {} }),
            0.85,
        );
    }

    // Local IP address
    if words
        .iter()
        .any(|w| ["ip", "ipv4", "address"].contains(w))
        && !words.iter().any(|w| ["ipv6"].contains(w))
    {
        return intent(
            Command::Explore,
            json!({ "probe": "local_ip", "probe_args": {} }),
            0.8,
        );
    }

    // GPU / graphics card
    if words
        .iter()
        .any(|w| ["gpu", "graphics", "video"].contains(w))
    {
        return intent(
            Command::Explore,
            json!({ "probe": "gpu_info", "probe_args": {} }),
            0.85,
        );
    }

    // Battery / charge
    if words
        .iter()
        .any(|w| ["battery", "charge", "charging"].contains(w))
    {
        return intent(
            Command::Explore,
            json!({ "probe": "battery_status", "probe_args": {} }),
            0.85,
        );
    }

    // SSH keys
    if words.contains(&"ssh")
        && words.iter().any(|w| ["key", "keys"].contains(w))
    {
        return intent(
            Command::Explore,
            json!({ "probe": "ssh_key_exists", "probe_args": {} }),
            0.85,
        );
    }

    // WSL (Windows Subsystem for Linux)
    if words.contains(&"wsl")
        || (words.contains(&"linux")
            && words
                .iter()
                .any(|w| ["windows", "subsystem", "wsl"].contains(w)))
    {
        return intent(
            Command::Explore,
            json!({ "probe": "wsl_version", "probe_args": {} }),
            0.85,
        );
    }

    // Windows package managers
    if words.contains(&"winget") {
        return intent(
            Command::Explore,
            json!({ "probe": "winget_version", "probe_args": {} }),
            0.9,
        );
    }
    if words.contains(&"scoop") {
        return intent(
            Command::Explore,
            json!({ "probe": "scoop_version", "probe_args": {} }),
            0.9,
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

    // Env var — matches explicit keywords, %VAR%, $VAR, and "what is X" where X
    // looks like an env var name.
    if words.iter().any(|w| {
        ["env", "environment", "variable"].contains(w)
            || (w.starts_with('%') && w.ends_with('%') && w.len() > 2)
            || (w.starts_with('$') && w.len() > 1)
    }) || (words.first() == Some(&"what")
        && words.get(1) == Some(&"is")
        && words.get(2).map(|w| {
            (w.starts_with('%') && w.ends_with('%'))
                || w.chars().all(|c| c.is_uppercase() || c == '_')
        }).unwrap_or(false))
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

    // Generic "is X installed?" — catch-all for named apps not covered by specific probes.
    // Confidence is intentionally low (0.65) to avoid false positives.
    if words
        .iter()
        .any(|w| ["installed", "available", "present"].contains(w))
    {
        let stop: &[&str] = &[
            "i", "a", "the", "is", "do", "have", "got", "any", "installed", "available",
            "present", "on", "my", "this", "machine", "computer", "it", "yet", "already",
            "system", "here",
        ];
        if let Some(name) = words.iter().find(|&&w| !stop.contains(&w)) {
            return intent(
                Command::Explore,
                json!({ "probe": "installed_app", "probe_args": { "name": name } }),
                0.65,
            );
        }
    }

    // No specific probe matched — return Explore with an empty probe name so
    // the brain's `run_probe` path can run `suggest_for_explore` on the raw
    // input and return keyword-scored suggestions instead of a dead end.
    intent(Command::Explore, json!({ "probe": "" }), 0.3)
}

/// Returns the `Expand` intent, with an optional follow-up question.
pub fn resolve_expand(words: &[&str]) -> Intent {
    let question: Option<String> = if words.is_empty() {
        None
    } else {
        Some(words.join(" "))
    };
    intent(Command::Expand, json!({ "question": question }), 1.0)
}

// ── Helpers ────────────────────────────────────────────────────────────────

/// Returns `true` if the word looks like a package identifier (not a common
/// English pronoun, article, or preposition that would indicate natural language).
fn looks_like_id(word: &str) -> bool {
    const NOT_ID: &[&str] = &[
        "me", "my", "your", "their", "our", "its", "his", "her",
        "the", "a", "an", "some", "any", "all",
        "this", "that", "these", "those",
        "what", "which", "how", "why", "when", "where", "who", "whom",
        "i", "you", "he", "she", "we", "they",
        "is", "are", "was", "were", "be", "been", "being",
        "have", "has", "had", "do", "does", "did",
    ];
    !word.is_empty() && !NOT_ID.contains(&word)
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
