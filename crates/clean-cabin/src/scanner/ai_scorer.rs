//! AI-assisted file scoring using genai directly (no lodge-brain dependency).
//!
//! Provider resolution order (Auto mode):
//!   1. Ollama — TCP-probe 127.0.0.1:11434 with 400 ms timeout
//!   2. Cloud — LODGE_GEMINI_KEY → Gemini; LODGE_CLAUDE_KEY → Claude
//!   3. Heuristics-only fallback (YouDecide for all entries)

use std::net::TcpStream;
use std::time::Duration;

use crate::config::{AiMode, Config};
use crate::scanner::heuristics::TierHint;
use crate::scanner::walker::FileEntry;

/// AI scoring result for a single file.
#[derive(Debug, Clone)]
pub struct AiScore {
    /// AI-produced tier hint.
    pub tier_hint: TierHint,
    /// One-sentence reason from the model.
    pub reason: String,
}

// ── Provider detection ────────────────────────────────────────────────────────

/// Returns `true` if Ollama is reachable on localhost:11434.
fn ollama_reachable() -> bool {
    TcpStream::connect_timeout(
        &"127.0.0.1:11434".parse().unwrap(),
        Duration::from_millis(400),
    )
    .is_ok()
}

/// Returns the Gemini API key if set.
fn gemini_key() -> Option<String> {
    std::env::var("LODGE_GEMINI_KEY").ok().filter(|s| !s.is_empty())
}

/// Returns the Claude (Anthropic) API key if set.
fn claude_key() -> Option<String> {
    std::env::var("LODGE_CLAUDE_KEY").ok().filter(|s| !s.is_empty())
}

// ── Ollama call ───────────────────────────────────────────────────────────────

/// Call Ollama's generate endpoint synchronously.
fn call_ollama(prompt: &str, ollama_url: &str) -> Option<String> {
    let url = format!("{ollama_url}/api/generate");
    let body = serde_json::json!({
        "model": "llama3",
        "prompt": prompt,
        "stream": false,
    });

    let client = reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(30))
        .build()
        .ok()?;

    let resp = client.post(&url).json(&body).send().ok()?;
    let json: serde_json::Value = resp.json().ok()?;
    json.get("response")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
}

// ── genai cloud call ──────────────────────────────────────────────────────────

/// Call a cloud provider via genai synchronously using a blocking tokio runtime.
fn call_genai(prompt: &str, model: &str, api_key: &str, key_env: &str) -> Option<String> {
    use genai::chat::{ChatMessage, ChatRequest};

    // Set the key env var that genai reads.
    std::env::set_var(key_env, api_key);

    let rt = tokio::runtime::Runtime::new().ok()?;
    rt.block_on(async {
        let client = genai::Client::default();
        let request = ChatRequest::new(vec![ChatMessage::user(prompt)]);
        let response = client.exec_chat(model, request, None).await.ok()?;
        response
            .content_text_as_str()
            .map(|s| s.to_string())
    })
}

// ── Prompt builder ────────────────────────────────────────────────────────────

fn build_prompt(entries: &[&FileEntry]) -> String {
    use std::time::SystemTime;

    fn days_since(t: Option<SystemTime>) -> u64 {
        t.and_then(|st| SystemTime::now().duration_since(st).ok())
            .map(|d| d.as_secs() / 86_400)
            .unwrap_or(0)
    }

    let items: Vec<serde_json::Value> = entries
        .iter()
        .enumerate()
        .map(|(i, e)| {
            serde_json::json!({
                "index": i,
                "path": e.path.to_string_lossy(),
                "size_bytes": e.size,
                "days_since_modified": days_since(e.modified),
            })
        })
        .collect();

    let system = "You are a file cleanup assistant. Given a list of files, classify each as: \
        CLEAR_OUT (definitely junk), WORTH_A_LOOK (possibly junk), or KEEP (probably needed). \
        Return a JSON array with one object per file: \
        {\"index\": N, \"tier\": \"CLEAR_OUT\"|\"WORTH_A_LOOK\"|\"KEEP\", \"reason\": \"one sentence\"}. \
        Be concise.";

    format!(
        "{system}\n\nFiles:\n{}\n\nReturn only the JSON array.",
        serde_json::to_string_pretty(&items).unwrap_or_default()
    )
}

// ── Response parser ───────────────────────────────────────────────────────────

fn parse_response(raw: &str, count: usize) -> Vec<AiScore> {
    let array = extract_json_array(raw);
    let items: Vec<serde_json::Value> = match array.and_then(|s| serde_json::from_str(&s).ok()) {
        Some(v) => v,
        None => return fallback(count),
    };

    let mut scores: Vec<Option<AiScore>> = vec![None; count];

    for item in items {
        let index = item
            .get("index")
            .and_then(|v| v.as_u64())
            .unwrap_or(u64::MAX) as usize;
        if index >= count {
            continue;
        }
        let tier_str = item
            .get("tier")
            .and_then(|v| v.as_str())
            .unwrap_or("KEEP");
        let reason = item
            .get("reason")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();

        let tier_hint = match tier_str {
            "CLEAR_OUT" => TierHint::ClearOut,
            "WORTH_A_LOOK" => TierHint::WorthALook,
            _ => TierHint::Keep,
        };

        scores[index] = Some(AiScore { tier_hint, reason });
    }

    scores
        .into_iter()
        .map(|s| {
            s.unwrap_or_else(|| AiScore {
                tier_hint: TierHint::YouDecide,
                reason: String::new(),
            })
        })
        .collect()
}

/// Extract the first `[...]` JSON array from a string.
fn extract_json_array(s: &str) -> Option<String> {
    let start = s.find('[')?;
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
            '[' if !in_string => depth += 1,
            ']' if !in_string => {
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

// ── Fallback ──────────────────────────────────────────────────────────────────

fn fallback(count: usize) -> Vec<AiScore> {
    (0..count)
        .map(|_| AiScore {
            tier_hint: TierHint::YouDecide,
            reason: String::new(),
        })
        .collect()
}

// ── Public entry point ────────────────────────────────────────────────────────

/// Score a batch of up to 20 files using the configured AI provider.
///
/// Returns one `AiScore` per entry in `entries`. Falls back to `YouDecide`
/// for all entries if no provider is available or the response cannot be parsed.
pub fn score_batch(entries: &[&FileEntry], config: &Config) -> Vec<AiScore> {
    if config.ai_mode == AiMode::None {
        return fallback(entries.len());
    }

    // Cap to 20 files to stay within token budget.
    let batch = if entries.len() > 20 {
        &entries[..20]
    } else {
        entries
    };

    let prompt = build_prompt(batch);

    // Resolve which provider to use.
    let raw: Option<String> = match &config.ai_mode {
        AiMode::Ollama => call_ollama(&prompt, &config.ollama_url),

        AiMode::Api => {
            if let Some(key) = gemini_key() {
                call_genai(&prompt, "gemini-1.5-flash-latest", &key, "GEMINI_API_KEY")
            } else if let Some(key) = claude_key() {
                call_genai(&prompt, "claude-3-haiku-20240307", &key, "ANTHROPIC_API_KEY")
            } else {
                None
            }
        }

        AiMode::Auto => {
            // 1. Try Ollama.
            if ollama_reachable() {
                let result = call_ollama(&prompt, &config.ollama_url);
                if result.is_some() {
                    result
                } else {
                    try_cloud_providers(&prompt)
                }
            } else {
                try_cloud_providers(&prompt)
            }
        }

        AiMode::None => unreachable!("handled above"),
    };

    match raw {
        Some(text) => parse_response(&text, batch.len()),
        None => fallback(batch.len()),
    }
}

/// Try cloud providers in preference order (Gemini then Claude).
fn try_cloud_providers(prompt: &str) -> Option<String> {
    if let Some(key) = gemini_key() {
        let result = call_genai(prompt, "gemini-1.5-flash-latest", &key, "GEMINI_API_KEY");
        if result.is_some() {
            return result;
        }
    }
    if let Some(key) = claude_key() {
        return call_genai(prompt, "claude-3-haiku-20240307", &key, "ANTHROPIC_API_KEY");
    }
    None
}
