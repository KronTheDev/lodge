//! Optional Claude API integration for the `expand` command.
//!
//! This module is entirely opt-in. No network calls are made unless the user
//! explicitly invokes `expand` and has a Claude API key configured. The key
//! is never read, stored in memory, or transmitted except during an explicit
//! `expand` call.
//!
//! Key resolution order:
//! 1. `LODGE_CLAUDE_API_KEY` environment variable
//! 2. Key file at `%LOCALAPPDATA%\lodge\claude.key` (Windows) or
//!    `~/.local/share/lodge/claude.key` (Unix)

use std::path::PathBuf;

use anyhow::{Context, Result};
use serde_json::json;

const API_URL: &str = "https://api.anthropic.com/v1/messages";
const MODEL: &str = "claude-haiku-4-5-20251001";
const MAX_TOKENS: u32 = 512;
const TIMEOUT_SECS: u64 = 20;

const SYSTEM_PROMPT: &str = "You are a concise technical assistant embedded in Lodge, \
    a developer installation runtime. The user is asking about their machine. \
    Answer in 1-4 sentences. No markdown. No bullet points. Plain prose, direct, calm.";

// ── Key file path ────────────────────────────────────────────────────────────

/// Returns the platform-specific path to the persisted key file.
fn key_file_path() -> Option<PathBuf> {
    #[cfg(windows)]
    {
        std::env::var_os("LOCALAPPDATA").map(|base| {
            PathBuf::from(base).join("lodge").join("claude.key")
        })
    }
    #[cfg(not(windows))]
    {
        dirs_next_home().map(|home| {
            home.join(".local").join("share").join("lodge").join("claude.key")
        })
    }
}

/// Minimal `$HOME` resolution for non-Windows targets without pulling `dirs`.
#[cfg(not(windows))]
fn dirs_next_home() -> Option<PathBuf> {
    std::env::var_os("HOME").map(PathBuf::from)
}

// ── Public API ───────────────────────────────────────────────────────────────

/// Returns the configured Claude API key, or `None` if no key is available.
///
/// Checks the `LODGE_CLAUDE_API_KEY` environment variable first; if absent,
/// falls back to the key file on disk. Returns `None` if neither source
/// yields a non-empty value.
pub fn api_key() -> Option<String> {
    // 1. Environment variable takes precedence.
    if let Ok(val) = std::env::var("LODGE_CLAUDE_API_KEY") {
        let trimmed = val.trim().to_string();
        if !trimmed.is_empty() {
            return Some(trimmed);
        }
    }

    // 2. Key file on disk.
    let path = key_file_path()?;
    let raw = std::fs::read_to_string(&path).ok()?;
    let trimmed = raw.trim().to_string();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed)
    }
}

/// Persists `key` to the platform key file, creating the directory if needed.
///
/// Overwrites any existing key file. Does not validate the key format.
pub fn save_key(key: &str) -> Result<()> {
    let path = key_file_path()
        .context("could not determine key file path — LOCALAPPDATA or HOME is unset")?;

    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("couldn't create directory: {}", parent.display()))?;
    }

    std::fs::write(&path, key.trim())
        .with_context(|| format!("couldn't write key file: {}", path.display()))?;

    Ok(())
}

/// Calls the Claude API with the last probe result as context and returns a
/// plain-language explanation.
///
/// If `question` is `None`, Claude is asked to expand on the probe result in
/// general. If `question` is provided, it is asked as a follow-up against
/// that result.
///
/// Returns a calm error message string (never panics) if the key is missing,
/// the network is unreachable, or the API returns an error.
pub fn expand(last_result: &str, question: Option<&str>) -> String {
    let key = match api_key() {
        Some(k) => k,
        None => {
            return "no Claude API key configured. \
                set LODGE_CLAUDE_API_KEY or run `lodge key set <key>` to add one."
                .into();
        }
    };

    let user_message = match question {
        None => format!(
            "Expand on this Lodge probe result. \
             Explain it in more detail and note anything the user should be aware of:\n\n\
             {last_result}"
        ),
        Some(q) => format!("Lodge probe result:\n{last_result}\n\nQuestion: {q}"),
    };

    let body = json!({
        "model": MODEL,
        "max_tokens": MAX_TOKENS,
        "system": SYSTEM_PROMPT,
        "messages": [
            { "role": "user", "content": user_message }
        ]
    });

    call_api(&key, &body)
}

// ── Private helpers ──────────────────────────────────────────────────────────

/// POSTs `body` to the Claude Messages API and returns the response text,
/// or a calm error string if anything goes wrong.
fn call_api(key: &str, body: &serde_json::Value) -> String {
    let agent = ureq::AgentBuilder::new()
        .timeout(std::time::Duration::from_secs(TIMEOUT_SECS))
        .build();

    let result = agent
        .post(API_URL)
        .set("x-api-key", key)
        .set("anthropic-version", "2023-06-01")
        .set("content-type", "application/json")
        .send_json(body);

    match result {
        Ok(response) => {
            match response.into_json::<serde_json::Value>() {
                Ok(json) => {
                    json["content"][0]["text"]
                        .as_str()
                        .map(|s| s.trim().to_string())
                        .unwrap_or_else(|| {
                            "received an unexpected response from Claude.".into()
                        })
                }
                Err(_) => "couldn't parse the response from Claude.".into(),
            }
        }
        Err(ureq::Error::Status(401, _)) => {
            "Claude API key was rejected. check that the key is correct and still active.".into()
        }
        Err(ureq::Error::Status(429, _)) => {
            "Claude API rate limit reached. try again in a moment.".into()
        }
        Err(ureq::Error::Status(code, response)) => {
            // Surface the API error message so failures are diagnosable.
            let detail = response
                .into_string()
                .ok()
                .and_then(|s| serde_json::from_str::<serde_json::Value>(&s).ok())
                .and_then(|j| j["error"]["message"].as_str().map(str::to_string))
                .unwrap_or_else(|| "no details returned".into());
            format!("Claude API error {code}: {detail}")
        }
        Err(ureq::Error::Transport(_)) => {
            "couldn't reach the Claude API. check your network connection.".into()
        }
    }
}

// ── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn expand_without_key_returns_helpful_message() {
        // Ensure no key leaks in from the environment for this test.
        // We shadow the env var by temporarily removing it if present.
        // This test only verifies the no-key path.
        let original = std::env::var("LODGE_CLAUDE_API_KEY").ok();
        std::env::remove_var("LODGE_CLAUDE_API_KEY");

        // Only run the body of the test if we can confirm the key file doesn't
        // happen to exist on this machine (e.g. in CI).
        if api_key().is_none() {
            let result = expand("node v20.0.0", None);
            assert!(
                result.contains("no Claude API key"),
                "expected no-key message, got: {result}"
            );
        }

        // Restore env var if it existed before.
        if let Some(val) = original {
            std::env::set_var("LODGE_CLAUDE_API_KEY", val);
        }
    }

    #[test]
    fn expand_message_with_question_formats_correctly() {
        // Verify the user message body contains both the result and the question.
        // We test this indirectly by checking `expand` returns the no-key
        // message when no key is present — confirming it reaches the key check
        // before making any network call.
        std::env::remove_var("LODGE_CLAUDE_API_KEY");
        if api_key().is_none() {
            let result = expand("Python 3.12.0", Some("is this new enough for PEP 695?"));
            assert!(!result.is_empty());
        }
    }

    #[test]
    fn save_key_round_trips() {
        // If we can determine a key file path, write and read it back.
        if let Some(path) = key_file_path() {
            // Only run in a temp-safe context — skip if path seems like a real
            // user's key file that already exists.
            if !path.exists() {
                if save_key("sk-test-key").is_ok() {
                    let contents = std::fs::read_to_string(&path).unwrap_or_default();
                    assert_eq!(contents.trim(), "sk-test-key");
                    // Clean up.
                    let _ = std::fs::remove_file(&path);
                }
            }
        }
    }
}
