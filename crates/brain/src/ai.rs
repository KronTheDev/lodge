//! Multi-provider AI integration for `expand` and `scan` narration.
//!
//! Provider resolution order (first available wins):
//!   1. Ollama at localhost:11434  — local, no key, offline
//!   2. Gemini key (LODGE_GEMINI_KEY env or gemini.key file)
//!   3. Claude key (LODGE_CLAUDE_KEY env or claude.key file)
//!
//! Configuration is persisted to `%LOCALAPPDATA%\lodge\ai.toml` (Windows) or
//! `~/.local/share/lodge/ai.toml` (Unix) by the onboarding sequence and can be
//! updated at any time. Runtime resolution always re-reads the file so changes
//! take effect without restarting Lodge.
//!
//! ## Key storage security
//!
//! On Windows, API keys are encrypted at rest using DPAPI (`CryptProtectData`).
//! The encrypted blob is user-scoped and machine-scoped — only the same Windows
//! user account on the same machine can decrypt it.  The stored value in ai.toml
//! starts with the prefix `dpapi:` followed by the hex-encoded ciphertext.
//!
//! On non-Windows platforms the key is stored as plain text (file permissions
//! are the only protection, same as most other local dev tools).
//!
//! If decryption fails (different machine, migrated profile), the key is treated
//! as absent and the user is prompted to re-enter it.

use std::borrow::Cow;
use std::net::TcpStream;
use std::path::PathBuf;
use std::time::Duration;

use anyhow::{Context, Result};
use genai::chat::{ChatMessage, ChatRequest};
use serde::{Deserialize, Serialize};

// ── Model constants ───────────────────────────────────────────────────────────

pub const MODEL_OLLAMA:   &str = "llama3.2:3b";
pub const MODEL_GEMINI:   &str = "gemini-2.5-flash";
pub const MODEL_CLAUDE:   &str = "claude-3-5-haiku-20241022";
pub const MODEL_OPENAI:   &str = "gpt-4o-mini";
pub const MODEL_GROQ:     &str = "llama-3.3-70b-versatile";
pub const MODEL_XAI:      &str = "grok-3-mini";
pub const MODEL_DEEPSEEK: &str = "deepseek-chat";
pub const MODEL_COHERE:   &str = "command-r";

const OLLAMA_HOST: &str = "127.0.0.1:11434";
const OLLAMA_PROBE_TIMEOUT: Duration = Duration::from_millis(400);

const SYSTEM_PROMPT: &str = "You are a concise technical assistant embedded in Lodge, \
    a developer installation runtime. The user is asking about their machine or a \
    system probe result. Answer in 1-4 sentences. No markdown. No bullet points. \
    Plain prose, direct, calm.";

// ── DPAPI encryption (Windows only) ──────────────────────────────────────────

/// Windows DPAPI key encryption.  Keys stored in ai.toml are prefixed with
/// `dpapi:` and hex-encoded so the file remains plain-text parseable TOML.
///
/// On non-Windows builds this module is absent; the stubs in `load_config` /
/// `save_config` compile away via `#[cfg]`.
#[cfg(windows)]
mod dpapi {
    use std::ffi::c_void;

    /// Wire format prefix that marks an encrypted blob in ai.toml.
    pub const PREFIX: &str = "dpapi:";

    #[repr(C)]
    struct DataBlob {
        cb_data: u32,
        pb_data: *mut u8,
    }

    // CryptProtectData / CryptUnprotectData live in Crypt32.lib.
    #[link(name = "Crypt32")]
    unsafe extern "system" {
        fn CryptProtectData(
            p_data_in:         *const DataBlob,
            sz_data_descr:     *const u16,
            p_entropy:         *const DataBlob,
            pv_reserved:       *mut c_void,
            p_prompt:          *const c_void,
            dw_flags:          u32,
            p_data_out:        *mut DataBlob,
        ) -> i32;

        fn CryptUnprotectData(
            p_data_in:         *const DataBlob,
            pp_descr:          *mut *mut u16,
            p_entropy:         *const DataBlob,
            pv_reserved:       *mut c_void,
            p_prompt:          *const c_void,
            dw_flags:          u32,
            p_data_out:        *mut DataBlob,
        ) -> i32;
    }

    // LocalFree is in kernel32 (linked by default on Windows).
    unsafe extern "system" {
        fn LocalFree(h_mem: *const c_void) -> *const c_void;
    }

    /// No-UI flag — DPAPI will never show a dialog.
    const UI_FORBIDDEN: u32 = 0x1;

    fn to_hex(b: &[u8]) -> String {
        b.iter().map(|x| format!("{x:02x}")).collect()
    }

    fn from_hex(s: &str) -> Option<Vec<u8>> {
        if !s.len().is_multiple_of(2) { return None; }
        (0..s.len()).step_by(2)
            .map(|i| u8::from_str_radix(&s[i..i + 2], 16).ok())
            .collect()
    }

    /// Encrypt `plaintext` with DPAPI.  Returns `Some("dpapi:<hex>")` or
    /// `None` if DPAPI is unavailable (e.g. in a sandboxed CI environment).
    pub fn protect(plaintext: &str) -> Option<String> {
        if plaintext.is_empty() { return Some(String::new()); }
        let bytes = plaintext.as_bytes();
        let input  = DataBlob { cb_data: bytes.len() as u32, pb_data: bytes.as_ptr() as *mut u8 };
        let mut output = DataBlob { cb_data: 0, pb_data: std::ptr::null_mut() };
        unsafe {
            let ok = CryptProtectData(
                &input,
                std::ptr::null(), std::ptr::null(),
                std::ptr::null_mut(), std::ptr::null(),
                UI_FORBIDDEN,
                &mut output,
            );
            if ok == 0 { return None; }
            let slice = std::slice::from_raw_parts(output.pb_data, output.cb_data as usize);
            let result = format!("{}{}", PREFIX, to_hex(slice));
            LocalFree(output.pb_data as *const c_void);
            Some(result)
        }
    }

    /// Decrypt a `dpapi:<hex>` blob.  Returns the plaintext, or `None` if the
    /// blob is malformed or was encrypted by a different user / machine.
    pub fn unprotect(stored: &str) -> Option<String> {
        let hex = stored.strip_prefix(PREFIX)?;
        let bytes = from_hex(hex)?;
        let input  = DataBlob { cb_data: bytes.len() as u32, pb_data: bytes.as_ptr() as *mut u8 };
        let mut output = DataBlob { cb_data: 0, pb_data: std::ptr::null_mut() };
        unsafe {
            let ok = CryptUnprotectData(
                &input,
                std::ptr::null_mut(), std::ptr::null(),
                std::ptr::null_mut(), std::ptr::null(),
                UI_FORBIDDEN,
                &mut output,
            );
            if ok == 0 { return None; }
            let slice = std::slice::from_raw_parts(output.pb_data, output.cb_data as usize);
            let plaintext = String::from_utf8(slice.to_vec()).ok()?;
            LocalFree(output.pb_data as *const c_void);
            Some(plaintext)
        }
    }

    /// Returns `true` if `s` is a DPAPI-encrypted blob (not plaintext).
    pub fn is_protected(s: &str) -> bool {
        s.starts_with(PREFIX)
    }
}

// ── Provider definitions ──────────────────────────────────────────────────────

/// Static metadata for a genai-supported AI provider.
pub struct ProviderDef {
    pub id: &'static str,
    pub name: &'static str,
    pub default_model: &'static str,
    /// Environment variable genai reads for this provider's API key.
    pub key_env: &'static str,
    /// Brief UI hint shown during onboarding (where to get a key, cost note).
    pub key_hint: &'static str,
    /// Whether this provider requires an API key.
    pub needs_key: bool,
}

/// All genai-supported providers, in display order.
pub const PROVIDERS: &[ProviderDef] = &[
    ProviderDef { id: "none",      name: "none",     default_model: "",                          key_env: "",                  key_hint: "set up later with  `!key`",              needs_key: false },
    ProviderDef { id: "ollama",    name: "Ollama",   default_model: "llama3.2:3b",               key_env: "",                  key_hint: "local · free · install from ollama.com", needs_key: false },
    ProviderDef { id: "gemini",    name: "Gemini",   default_model: "gemini-2.5-flash",          key_env: "GEMINI_API_KEY",    key_hint: "aistudio.google.com · free tier",        needs_key: true  },
    ProviderDef { id: "anthropic", name: "Claude",   default_model: "claude-3-5-haiku-20241022", key_env: "ANTHROPIC_API_KEY", key_hint: "console.anthropic.com",                  needs_key: true  },
    ProviderDef { id: "openai",    name: "OpenAI",   default_model: "gpt-4o-mini",               key_env: "OPENAI_API_KEY",    key_hint: "platform.openai.com",                    needs_key: true  },
    ProviderDef { id: "groq",      name: "Groq",     default_model: "llama-3.3-70b-versatile",   key_env: "GROQ_API_KEY",      key_hint: "console.groq.com · free tier · fast",    needs_key: true  },
    ProviderDef { id: "xai",       name: "xAI",      default_model: "grok-3-mini",               key_env: "XAI_API_KEY",       key_hint: "console.x.ai",                           needs_key: true  },
    ProviderDef { id: "deepseek",  name: "DeepSeek", default_model: "deepseek-chat",             key_env: "DEEPSEEK_API_KEY",  key_hint: "platform.deepseek.com · very cheap",     needs_key: true  },
    ProviderDef { id: "cohere",    name: "Cohere",   default_model: "command-r",                 key_env: "CO_API_KEY",        key_hint: "dashboard.cohere.com · free tier",       needs_key: true  },
    ProviderDef { id: "custom",    name: "custom",   default_model: "",                          key_env: "",                  key_hint: "enter any genai model string + API key", needs_key: true  },
];

// ── Config file ───────────────────────────────────────────────────────────────

/// Persisted AI configuration written by the onboarding sequence.
///
/// `key` is always plaintext in memory; encryption/decryption happens
/// only at the `save_config` / `load_config` boundary.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct AiConfig {
    /// The genai model string that encodes the provider by prefix.
    /// Empty string means "no provider configured".
    pub model: String,
    /// API key (plaintext), empty for Ollama.
    #[serde(default)]
    pub key: String,
    /// Provider identifier stored to disambiguate models with overlapping names.
    /// "ollama" | "gemini" | "anthropic" | "openai" | "groq" | "xai" | "deepseek" | "cohere" | ""
    #[serde(default)]
    pub provider: String,
}

impl AiConfig {
    pub fn is_empty(&self) -> bool {
        self.model.is_empty()
    }
}

fn config_path() -> Option<PathBuf> {
    #[cfg(windows)]
    {
        std::env::var_os("LOCALAPPDATA")
            .map(|b| PathBuf::from(b).join("lodge").join("ai.toml"))
    }
    #[cfg(not(windows))]
    {
        std::env::var_os("HOME")
            .map(|h| PathBuf::from(h).join(".local").join("share").join("lodge").join("ai.toml"))
    }
}

/// Load the persisted AI config.
///
/// On Windows, DPAPI-encrypted keys are transparently decrypted.  If
/// decryption fails (migrated machine, corrupt blob) the key is treated as
/// absent so the user is prompted to re-enter it.
pub fn load_config() -> AiConfig {
    let mut cfg: AiConfig = config_path()
        .and_then(|p| std::fs::read_to_string(p).ok())
        .and_then(|s| toml::from_str(&s).ok())
        .unwrap_or_default();

    #[cfg(windows)]
    if dpapi::is_protected(&cfg.key) {
        // Decryption failure → empty key (user must re-enter).
        cfg.key = dpapi::unprotect(&cfg.key).unwrap_or_default();
    }

    cfg
}

/// Persist an AI config to disk, creating directories as needed.
///
/// On Windows the `key` field is encrypted with DPAPI before writing.
/// Falls back to plaintext if DPAPI is unavailable (sandboxed environments).
pub fn save_config(cfg: &AiConfig) -> Result<()> {
    // Encrypt the key on Windows before touching the file.
    #[cfg(windows)]
    let stored_key = if cfg.key.is_empty() {
        String::new()
    } else {
        dpapi::protect(&cfg.key).unwrap_or_else(|| cfg.key.clone())
    };
    #[cfg(not(windows))]
    let stored_key = cfg.key.clone();

    let on_disk = AiConfig { model: cfg.model.clone(), key: stored_key, provider: cfg.provider.clone() };

    let path = config_path()
        .context("could not determine ai.toml path")?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("couldn't create {}", parent.display()))?;
    }
    let toml = toml::to_string_pretty(&on_disk)
        .context("couldn't serialise ai config")?;
    std::fs::write(&path, toml)
        .with_context(|| format!("couldn't write {}", path.display()))?;
    Ok(())
}

// ── Provider detection ────────────────────────────────────────────────────────

/// Returns `true` if Ollama is reachable at localhost:11434.
pub fn ollama_reachable() -> bool {
    TcpStream::connect_timeout(
        &OLLAMA_HOST.parse().expect("static addr"),
        OLLAMA_PROBE_TIMEOUT,
    )
    .is_ok()
}

#[derive(Debug, Clone, PartialEq)]
pub enum Provider {
    Ollama,
    Gemini,
    Claude,
    OpenAI,
    Groq,
    XAI,
    DeepSeek,
    Cohere,
    None,
}

impl Provider {
    pub fn label(&self) -> &'static str {
        match self {
            Provider::Ollama   => "Ollama (local)",
            Provider::Gemini   => "Gemini",
            Provider::Claude   => "Claude",
            Provider::OpenAI   => "OpenAI",
            Provider::Groq     => "Groq",
            Provider::XAI      => "xAI",
            Provider::DeepSeek => "DeepSeek",
            Provider::Cohere   => "Cohere",
            Provider::None     => "none",
        }
    }

    /// Convert a provider ID string (as stored in ai.toml) to Provider.
    pub fn from_id(id: &str) -> Self {
        match id {
            "ollama"    => Provider::Ollama,
            "gemini"    => Provider::Gemini,
            "anthropic" => Provider::Claude,
            "openai"    => Provider::OpenAI,
            "groq"      => Provider::Groq,
            "xai"       => Provider::XAI,
            "deepseek"  => Provider::DeepSeek,
            "cohere"    => Provider::Cohere,
            _           => Provider::None,
        }
    }
}

/// Resolve which provider and model to use at call time.
///
/// Resolution order:
///   1. Env var overrides (LODGE_GEMINI_KEY, LODGE_CLAUDE_KEY)
///   2. Saved ai.toml config (set during onboarding or `!key`)
///   3. Ollama reachability check
///   4. None
pub fn resolve_provider() -> (Provider, String, String) {
    // Env var overrides (highest priority)
    if let Ok(k) = std::env::var("LODGE_GEMINI_KEY") {
        let k = k.trim().to_string();
        if !k.is_empty() { return (Provider::Gemini, MODEL_GEMINI.into(), k); }
    }
    if let Ok(k) = std::env::var("LODGE_CLAUDE_KEY") {
        let k = k.trim().to_string();
        if !k.is_empty() { return (Provider::Claude, MODEL_CLAUDE.into(), k); }
    }

    // Saved config
    let cfg = load_config();
    if !cfg.model.is_empty() {
        let provider = if !cfg.provider.is_empty() {
            Provider::from_id(&cfg.provider)
        } else {
            infer_provider_from_model(&cfg.model)
        };

        // Migrate stale Gemini model name
        let model = if provider == Provider::Gemini && cfg.model != MODEL_GEMINI {
            let updated = AiConfig { model: MODEL_GEMINI.to_string(), key: cfg.key.clone(), provider: cfg.provider.clone() };
            let _ = save_config(&updated);
            MODEL_GEMINI.to_string()
        } else {
            cfg.model
        };

        return (provider, model, cfg.key);
    }

    // Fallback: check if Ollama is running
    if ollama_reachable() {
        return (Provider::Ollama, MODEL_OLLAMA.into(), String::new());
    }

    (Provider::None, String::new(), String::new())
}

fn infer_provider_from_model(model: &str) -> Provider {
    if model.contains(':') { return Provider::Ollama; }
    if model.starts_with("gemini")    { return Provider::Gemini; }
    if model.starts_with("claude")    { return Provider::Claude; }
    if model.starts_with("gpt") || model.starts_with("o1") || model.starts_with("o3") || model.starts_with("o4") {
        return Provider::OpenAI;
    }
    if model.starts_with("grok")      { return Provider::XAI; }
    if model.starts_with("deepseek")  { return Provider::DeepSeek; }
    if model.starts_with("command")   { return Provider::Cohere; }
    // llama-* without colon → could be Groq or Ollama; default Ollama
    Provider::Ollama
}

// ── Key helpers ───────────────────────────────────────────────────────────────

/// Detect provider from key prefix and save to ai.toml.
/// Gemini keys start with "AI", Claude keys start with "sk-ant-".
pub fn save_key(key: &str) -> Result<Provider> {
    let key = key.trim().to_string();
    let (provider, model) = if key.starts_with("sk-ant-") {
        (Provider::Claude, MODEL_CLAUDE.to_string())
    } else if key.starts_with("AI") {
        (Provider::Gemini, MODEL_GEMINI.to_string())
    } else {
        anyhow::bail!(
            "unrecognised key format. use  `!key set <provider> <model> <key>`  for providers without auto-detected key formats."
        );
    };
    let provider_id = match &provider {
        Provider::Claude => "anthropic",
        Provider::Gemini => "gemini",
        _ => "",
    };
    save_config(&AiConfig { model, key, provider: provider_id.to_string() })?;
    Ok(provider)
}

/// Save Ollama as the provider (no key needed).
pub fn save_ollama() -> Result<()> {
    save_config(&AiConfig {
        model: MODEL_OLLAMA.to_string(),
        key: String::new(),
        provider: "ollama".to_string(),
    })
}

/// Save an explicit provider + model + key combination.
///
/// Use this when the provider cannot be inferred unambiguously from the key
/// prefix alone (e.g. Groq uses llama model names that look like Ollama).
pub fn save_model_key(provider_id: &str, model: &str, key: &str) -> anyhow::Result<()> {
    save_config(&AiConfig {
        model: model.to_string(),
        key: key.to_string(),
        provider: provider_id.to_string(),
    })
}

/// Clear the saved config (resets to "no provider").
pub fn clear_config() -> Result<()> {
    save_config(&AiConfig::default())
}

// ── AI calls ─────────────────────────────────────────────────────────────────

/// Expand on a single probe result, optionally answering a follow-up question.
///
/// Runs the genai call on a one-shot tokio runtime (caller is a `thread::spawn`
/// background thread and is not already inside a tokio context).
pub fn expand(last_result: &str, question: Option<&str>) -> String {
    let (provider, model, key) = resolve_provider();
    if provider == Provider::None {
        return "no AI provider configured. \
            run `lodge help ai` to set one up, or install Ollama for a free local option."
            .into();
    }

    let user_msg = match question {
        None => format!(
            "Expand on this system probe result. Explain it and note anything the user should know:\n\n{last_result}"
        ),
        Some(q) => format!("Probe result:\n{last_result}\n\nQuestion: {q}"),
    };

    call(model, key, provider, SYSTEM_PROMPT, user_msg)
}

/// Narrate a full scan result.
pub fn narrate_scan(scan_text: &str) -> String {
    let (provider, model, key) = resolve_provider();
    if provider == Provider::None {
        return String::new(); // scan still shows its table; narration silently omitted
    }

    // Cap the scan input to ~1500 chars to avoid blowing the free-tier
    // per-minute token limit. All meaningful probe values fit well within that.
    const MAX_SCAN_CHARS: usize = 1500;
    let truncated;
    let payload = if scan_text.len() > MAX_SCAN_CHARS {
        truncated = format!("{}\n[...truncated]", &scan_text[..MAX_SCAN_CHARS]);
        &truncated
    } else {
        scan_text
    };

    let user_msg = format!(
        "System snapshot from a developer's machine. \
         Note what's interesting, what might be missing, anything worth flagging. \
         Brief, calm, plain prose — no lists:\n\n{payload}"
    );

    call(model, key, provider, SYSTEM_PROMPT, user_msg)
}

fn call(model: String, key: String, provider: Provider, system: &str, user_msg: String) -> String {
    let rt = match tokio::runtime::Runtime::new() {
        Ok(r) => r,
        Err(_) => return "couldn't start async runtime for AI call.".into(),
    };

    rt.block_on(async move {
        // Set the API key in the environment where genai expects it
        if !key.is_empty() {
            let env_var = match provider {
                Provider::Gemini   => "GEMINI_API_KEY",
                Provider::Claude   => "ANTHROPIC_API_KEY",
                Provider::OpenAI   => "OPENAI_API_KEY",
                Provider::Groq     => "GROQ_API_KEY",
                Provider::XAI      => "XAI_API_KEY",
                Provider::DeepSeek => "DEEPSEEK_API_KEY",
                Provider::Cohere   => "CO_API_KEY",
                _ => "",
            };
            if !env_var.is_empty() {
                std::env::set_var(env_var, &key);
            }
        }

        let client = genai::Client::default();
        let request = ChatRequest::new(vec![
            ChatMessage::system(system),
            ChatMessage::user(user_msg),
        ]);

        // One retry on 503 (transient server overload) after a brief pause.
        let mut last_err: Option<genai::Error> = None;
        for attempt in 0..2u8 {
            if attempt > 0 {
                tokio::time::sleep(std::time::Duration::from_secs(3)).await;
            }
            match client.exec_chat(&model, request.clone(), None).await {
                Ok(response) => {
                    return response
                        .first_text()
                        .unwrap_or("no response from model.")
                        .trim()
                        .to_string();
                }
                Err(e) if attempt == 0 && is_transient(&e) => {
                    last_err = Some(e);
                }
                Err(e) => return classify_err(&e).into_owned(),
            }
        }
        classify_err(&last_err.expect("set in loop")).into_owned()
    })
}

/// Returns `true` if the error is a transient server condition worth retrying.
fn is_transient(e: &genai::Error) -> bool {
    use genai::Error as E;
    use genai::webc::Error as W;
    match e {
        E::WebModelCall { webc_error, .. } | E::WebAdapterCall { webc_error, .. } => {
            matches!(webc_error, W::ResponseFailedStatus { status, .. } if status.as_u16() == 503)
        }
        E::HttpError { status, .. } => status.as_u16() == 503,
        _ => false,
    }
}

/// Classify a genai error into a user-facing message.
///
/// Uses direct enum-variant matching — zero string scanning, zero clones for
/// static messages. `Cow::Owned` is only constructed when we need to embed a
/// dynamic value (e.g. an unexpected HTTP status code).
fn classify_err(e: &genai::Error) -> Cow<'static, str> {
    use genai::Error as E;
    use genai::webc::Error as W;

    match e {
        // Auth failures — bad or missing key
        E::RequiresApiKey { .. } | E::NoAuthResolver { .. } | E::NoAuthData { .. } => {
            Cow::Borrowed("AI key rejected. check the key is correct and active.")
        }

        // Web-layer errors — dig into the webc sub-error
        E::WebModelCall { webc_error, .. } | E::WebAdapterCall { webc_error, .. } => {
            match webc_error {
                W::ResponseFailedStatus { status, body, .. } => {
                    classify_status(status.as_u16(), body)
                }
                W::Reqwest(_) => Cow::Borrowed(
                    "couldn't reach the AI provider. \
                     check your network or that Ollama is running.",
                ),
                _ => Cow::Owned(format!("AI error: {e}")),
            }
        }

        // Direct HTTP error (body not available here — fall back to code only)
        E::HttpError { status, .. } => classify_status(status.as_u16(), ""),

        // Everything else — surface raw for diagnosability
        _ => Cow::Owned(format!("AI error: {e}")),
    }
}

/// Map an HTTP status code (and optional response body) to a user-facing message.
///
/// The body is used to distinguish Gemini quota exhaustion (`RESOURCE_EXHAUSTED`)
/// from a transient rate limit — both surface as 429 but need different guidance.
fn classify_status(code: u16, body: &str) -> Cow<'static, str> {
    match code {
        401 | 403 => Cow::Borrowed("AI key rejected. check the key is correct and active."),
        429 if body.contains("retryDelay") => {
            // Per-minute rate limit — retry delay is short (seconds)
            Cow::Borrowed(
                "AI rate limited — free tier has a per-minute cap. \
                 wait a moment and try again.",
            )
        }
        429 => {
            // Daily quota exhausted — no short retry
            Cow::Borrowed(
                "AI quota exhausted for today. \
                 try again tomorrow, or switch to Ollama for unlimited local inference.",
            )
        }
        503 => Cow::Borrowed(
            "AI provider is temporarily overloaded (503). \
             retried once — still down. try again in a moment, \
             or install Ollama for unlimited local inference.",
        ),
        _ => Cow::Owned(format!("AI error: HTTP {code}")),
    }
}
