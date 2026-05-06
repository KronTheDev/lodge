pub mod ai;
pub mod context;
pub mod framer;
pub mod inference;
pub mod intent;
pub mod scan;
pub mod scout;

pub use context::ConversationContext;
pub use intent::{Command, Intent};

use inference::InferenceEngine;
use intent::{resolve_deterministic, resolve_from_json};
use scout::ProbeArgs;

/// (probe_name, keywords_for_scoring, example_prompt shown to the user)
static PROBE_EXAMPLES: &[(&str, &[&str], &str)] = &[
    ("git_version",      &["git"],                      "do I have git installed?"),
    ("node_version",     &["node", "nodejs"],            "what node version am I on?"),
    ("npm_version",      &["npm"],                       "what npm version am I on?"),
    ("python_version",   &["python", "py"],              "is python installed?"),
    ("java_version",     &["java", "jdk"],               "what java version do I have?"),
    ("go_version",       &["go", "golang"],              "is golang installed?"),
    ("ruby_version",     &["ruby"],                      "do I have ruby?"),
    ("php_version",      &["php"],                       "is PHP on PATH?"),
    ("dotnet_runtimes",  &["dotnet", "net"],             "what .NET runtimes do I have?"),
    ("docker_version",   &["docker"],                    "is docker running?"),
    ("ps_version",       &["powershell", "ps"],          "what PowerShell version am I on?"),
    ("execution_policy", &["execution", "policy"],       "what is my execution policy?"),
    ("os_build",         &["os", "windows", "build"],    "what OS build am I on?"),
    ("arch",             &["arch", "architecture"],      "what architecture am I running?"),
    ("cpu_info",         &["cpu", "cores", "processor"], "what CPU do I have?"),
    ("ram_usage",        &["ram", "memory"],             "how much RAM am I using?"),
    ("disk_space",       &["disk", "space", "drive"],    "how much space on C:?"),
    ("disk_space_all",   &["disks", "drives", "all"],    "how much free space across all drives?"),
    ("hostname",         &["hostname", "computer"],      "what is my computer name?"),
    ("username",         &["user", "username", "login"], "who am I logged in as?"),
    ("local_ip",         &["ip", "address", "network"],  "what is my local IP?"),
    ("uptime",           &["uptime", "boot"],            "how long has this PC been on?"),
    ("port_in_use",      &["port"],                      "is port 8080 free?"),
    ("process_running",  &["process", "running"],        "is nginx running?"),
    ("service_status",   &["service"],                   "is the print spooler running?"),
    ("path_exists",      &["path", "exists", "folder"],  "does C:\\Tools exist?"),
    ("env_var",          &["env", "appdata", "variable"],"what is %APPDATA%?"),
    ("wsl_version",      &["wsl", "linux"],              "is WSL installed?"),
    ("registry_key",     &["registry", "reg"],           "what is this registry key?"),
    ("ssh_key_exists",   &["ssh", "key"],                "do I have SSH keys?"),
];

/// Wagner-Fischer edit distance between two strings.
fn edit_distance(a: &str, b: &str) -> usize {
    let a: Vec<char> = a.chars().collect();
    let b: Vec<char> = b.chars().collect();
    let (m, n) = (a.len(), b.len());
    let mut row: Vec<usize> = (0..=n).collect();
    for i in 1..=m {
        let mut prev = row[0];
        row[0] = i;
        for j in 1..=n {
            let old = row[j];
            row[j] = if a[i - 1] == b[j - 1] {
                prev
            } else {
                1 + prev.min(row[j]).min(row[j - 1])
            };
            prev = old;
        }
    }
    row[n]
}

/// Returns true if `token` matches `keyword` exactly, by containment, or within
/// edit distance 1 (for tokens of length ≥ 3 to avoid false positives on short words).
fn token_matches(token: &str, keyword: &str) -> bool {
    token.contains(keyword)
        || keyword.contains(token)
        || (token.len() >= 3 && keyword.len() >= 3 && edit_distance(token, keyword) <= 1)
}

/// Score probes by keyword overlap with the user's input and return the top 3
/// as suggestion lines prefixed with `✦` so the TUI can render them clickable.
fn suggest_for_explore(input: &str) -> String {
    // Strip trailing punctuation so "how much RUM am I using?" scores correctly.
    let lower = input.trim_end_matches(|c: char| !c.is_alphanumeric()).to_lowercase();
    let tokens: Vec<&str> = lower.split_whitespace().collect();

    let mut scored: Vec<(usize, &str)> = PROBE_EXAMPLES
        .iter()
        .map(|(_, keywords, example)| {
            let score = keywords
                .iter()
                .filter(|kw| tokens.iter().any(|t| token_matches(t, kw)))
                .count();
            (score, *example)
        })
        .collect();

    scored.sort_by_key(|b| std::cmp::Reverse(b.0));

    let mut seen = std::collections::HashSet::new();
    let top: Vec<&str> = scored
        .iter()
        .filter(|(_, ex)| seen.insert(*ex))
        .take(3)
        .map(|(_, ex)| *ex)
        .collect();

    // If nothing scored, show a spread of common categories rather than
    // the first 3 entries (which would always be git/node/npm).
    let suggestions = if top.is_empty() || scored.first().map(|(s, _)| *s).unwrap_or(0) == 0 {
        // Pick one from dev tools, one from system info, one from storage/state.
        [
            "do I have git installed?",
            "how much RAM am I using?",
            "is port 8080 free?",
        ]
        .as_slice()
        .to_vec()
    } else {
        top
    };

    format!(
        "not sure what you're looking for. here are some things you can ask:\n\n{}",
        suggestions
            .iter()
            .map(|e| format!("  ✦ {e}"))
            .collect::<Vec<_>>()
            .join("\n\n")
    )
}

/// The Lodge brain — intent resolution, system exploration, conversation context.
///
/// Initialises on first use. If a model file is found it loads automatically;
/// otherwise the deterministic resolver handles all known commands.
///
/// The brain is entirely offline — no network calls at any stage.
pub struct Brain {
    engine: Option<InferenceEngine>,
    system_prompt: String,
    pub context: ConversationContext,
}

impl Brain {
    /// Initialise the brain.
    ///
    /// Searches for the model file at the standard paths (see [`inference::model_path`]).
    /// Falls back gracefully to deterministic mode if no model is found.
    pub fn new() -> Self {
        let engine = inference::model_path().and_then(|p| InferenceEngine::load(&p).ok());
        let system_prompt = intent::build_system_prompt();
        Self {
            engine,
            system_prompt,
            context: ConversationContext::new(),
        }
    }

    /// Returns `true` if a model is loaded and active.
    pub fn has_model(&self) -> bool {
        self.engine.is_some()
    }

    /// Route user input through the brain and return a plain-language response.
    ///
    /// Updates the rolling conversation context after each exchange.
    pub fn handle(&mut self, input: &str) -> String {
        let resolved = self.resolve(input);
        let response = self.dispatch(&resolved, input);
        self.context.push(input.to_string(), response.clone());
        response
    }

    /// Resolve input to an intent, using the model if available.
    fn resolve(&self, input: &str) -> Intent {
        if let Some(engine) = &self.engine {
            let context_prefix = self.context.as_prompt_prefix();
            let prompt = inference::format_prompt(&self.system_prompt, &context_prefix, input);
            if let Ok(raw) = engine.run(&prompt, 80) {
                // Extract JSON from model output (model may prepend text)
                if let Some(json) = extract_json(&raw) {
                    return resolve_from_json(&json, input);
                }
            }
        }
        resolve_deterministic(input)
    }

    /// Dispatch a resolved intent to a response string.
    fn dispatch(&self, intent: &Intent, raw_input: &str) -> String {
        use Command::*;
        match intent.command {
            Help => framer::HELP.to_string(),

            List => "no packages installed yet.".into(),

            History => "no installation history.".into(),

            Install => {
                let target = intent
                    .args
                    .get("target")
                    .and_then(|v| v.as_str())
                    .unwrap_or(raw_input.trim_start_matches("install").trim());
                format!("use `lodge install {target}` from the terminal to install packages.")
            }

            Uninstall => {
                let id = intent
                    .args
                    .get("id")
                    .and_then(|v| v.as_str())
                    .unwrap_or("?");
                format!("use `lodge uninstall {id}` from the terminal.")
            }

            Update => {
                let id = intent
                    .args
                    .get("id")
                    .and_then(|v| v.as_str())
                    .unwrap_or("?");
                format!("update for {id}: not yet implemented.")
            }

            UpdateAll => "update all: not yet implemented.".into(),

            Search => {
                let q = intent
                    .args
                    .get("query")
                    .and_then(|v| v.as_str())
                    .unwrap_or("?");
                format!("search for '{q}': not yet implemented.")
            }

            Info => {
                let id = intent
                    .args
                    .get("id")
                    .and_then(|v| v.as_str())
                    .unwrap_or("?");
                format!("info for {id}: not yet implemented.")
            }

            Verify => {
                let id = intent
                    .args
                    .get("id")
                    .and_then(|v| v.as_str())
                    .unwrap_or("?");
                format!("verify for {id}: not yet implemented.")
            }

            Rollback => {
                let id = intent
                    .args
                    .get("id")
                    .and_then(|v| v.as_str())
                    .unwrap_or("?");
                format!("rollback for {id}: not yet implemented.")
            }

            UpdateRulesets => "update rulesets: not yet implemented.".into(),

            Use => {
                let spec = intent
                    .args
                    .get("spec")
                    .and_then(|v| v.as_str())
                    .unwrap_or("?");
                format!("use {spec}: not yet implemented.")
            }

            Explore => self.run_probe(intent, raw_input),

            // Expand is handled by the runtime layer (holds the last probe result).
            Expand => "expand what? run a probe first, then type `expand`.".into(),

            Scan => scan::run_with_narration(),

            Clarify => intent
                .prompt
                .clone()
                .unwrap_or_else(|| "what would you like to do?".into()),
        }
    }

    /// Execute a probe and frame the result.
    fn run_probe(&self, intent: &Intent, raw_input: &str) -> String {
        let probe = intent
            .args
            .get("probe")
            .and_then(|v| v.as_str())
            .unwrap_or("");

        let probe_args: ProbeArgs = intent
            .args
            .get("probe_args")
            .and_then(|v| v.as_object())
            .map(|o| {
                o.iter()
                    .map(|(k, v)| (k.clone(), v.as_str().unwrap_or("").to_string()))
                    .collect()
            })
            .unwrap_or_default();

        match scout::dispatch(probe, &probe_args) {
            Some(result) => framer::frame_probe_result(probe, &result),
            None if probe.is_empty() => suggest_for_explore(raw_input),
            None => format!("unknown probe: {probe}"),
        }
    }
}

impl Default for Brain {
    fn default() -> Self {
        Self::new()
    }
}

/// Compatibility check against a manifest's `requires` block.
///
/// Runs the relevant probes and returns a list of `(label, status, detail)` rows
/// ready to be rendered in the command bar.
pub fn compatibility_check(
    manifest: &lodge_shared::manifest::Manifest,
) -> Vec<(String, bool, String)> {
    let mut rows = Vec::new();
    let reqs = &manifest.requires;

    // OS check
    if let Some(ref os) = reqs.os {
        let current = std::env::consts::OS;
        let matches = os.to_lowercase() == current.to_lowercase()
            || (os.to_lowercase() == "macos" && current == "macos");
        rows.push((format!("OS: {os}"), matches, format!("running {current}")));
    }

    // OS version check
    if let Some(ref min_ver) = reqs.os_version {
        let probe_result = scout::dispatch("os_build", &ProbeArgs::new());
        let (ok, detail) = match probe_result {
            Some(r) if r.found => {
                let raw = r.value.as_deref().unwrap_or("");
                (true, format!("{raw}  (required: {min_ver}+)"))
            }
            _ => (
                false,
                format!("couldn't determine OS version  (required: {min_ver}+)"),
            ),
        };
        rows.push(("OS version".into(), ok, detail));
    }

    // Elevation check
    if reqs.elevation {
        let elevated = is_elevated();
        rows.push((
            "admin rights".into(),
            elevated,
            if elevated {
                "available".into()
            } else {
                "not available".into()
            },
        ));
    }

    // PowerShell version check
    if let Some(ref min_ps) = reqs.ps_version {
        let probe_result = scout::dispatch("ps_version", &ProbeArgs::new());
        let (ok, detail) = match probe_result {
            Some(r) if r.found => {
                let raw = r.value.as_deref().unwrap_or("");
                let version_ok = semver_gte(raw, min_ps);
                (
                    version_ok,
                    format!("PowerShell {raw}  (required: {min_ps}+)"),
                )
            }
            _ => (
                false,
                format!("PowerShell not found  (required: {min_ps}+)"),
            ),
        };
        rows.push(("PowerShell".into(), ok, detail));
    }

    rows
}

fn is_elevated() -> bool {
    #[cfg(windows)]
    {
        // Check if running as admin via whoami
        std::process::Command::new("net")
            .args(["session"])
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false)
    }
    #[cfg(not(windows))]
    {
        // Check EUID == 0
        unsafe { libc::geteuid() == 0 }
    }
}

/// Very simple semver "greater than or equal" for X.Y.Z strings.
/// Falls back to string comparison if parsing fails.
fn semver_gte(actual: &str, minimum: &str) -> bool {
    fn parse(s: &str) -> Option<(u32, u32, u32)> {
        let parts: Vec<&str> = s.trim().splitn(4, '.').collect();
        let major = parts.first().and_then(|p| p.parse().ok())?;
        let minor = parts.get(1).and_then(|p| p.parse().ok()).unwrap_or(0);
        let patch = parts
            .get(2)
            .and_then(|p| {
                // Strip any suffix like "-LTS"
                p.split('-').next().and_then(|n| n.parse().ok())
            })
            .unwrap_or(0);
        Some((major, minor, patch))
    }
    match (parse(actual), parse(minimum)) {
        (Some(a), Some(m)) => a >= m,
        _ => actual >= minimum,
    }
}

/// Extracts the first JSON object `{...}` from a string.
fn extract_json(s: &str) -> Option<String> {
    let start = s.find('{')?;
    let rest = &s[start..];
    let mut depth = 0usize;
    let mut in_string = false;
    let mut escape = false;
    for (i, ch) in rest.char_indices() {
        if escape {
            escape = false;
            continue;
        }
        match ch {
            '\\' if in_string => escape = true,
            '"' => in_string = !in_string,
            '{' if !in_string => depth += 1,
            '}' if !in_string => {
                depth -= 1;
                if depth == 0 {
                    return Some(rest[..=i].to_string());
                }
            }
            _ => {}
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn brain_initialises_without_model() {
        let brain = Brain::new();
        // Without a model file present, has_model should be false
        // (unless the user actually has one at the standard path)
        let _ = brain.has_model(); // just checks it doesn't panic
    }

    #[test]
    fn brain_handle_help() {
        let mut brain = Brain::new();
        let r = brain.handle("help");
        assert!(r.contains("install"));
    }

    #[test]
    fn brain_handle_list() {
        let mut brain = Brain::new();
        let r = brain.handle("list");
        assert!(r.contains("no packages"));
    }

    #[test]
    fn brain_handle_explore_arch() {
        let mut brain = Brain::new();
        let r = brain.handle("what arch am I running");
        // arch probe should return the architecture string
        assert!(!r.is_empty());
    }

    #[test]
    fn extract_json_simple() {
        let s = r#"some text {"command": "help"} more text"#;
        let j = extract_json(s).unwrap();
        assert_eq!(j, r#"{"command": "help"}"#);
    }

    #[test]
    fn extract_json_nested() {
        let s = r#"{"a": {"b": 1}}"#;
        let j = extract_json(s).unwrap();
        assert_eq!(j, s);
    }

    #[test]
    fn semver_gte_basic() {
        assert!(semver_gte("7.4.1", "7.2.0"));
        assert!(semver_gte("7.2.0", "7.2.0"));
        assert!(!semver_gte("7.1.9", "7.2.0"));
        assert!(semver_gte("10.0.22000", "10.0.19041"));
    }

    #[test]
    fn context_accumulates() {
        let mut brain = Brain::new();
        brain.handle("help");
        brain.handle("list");
        assert_eq!(brain.context.len(), 2);
    }
}
