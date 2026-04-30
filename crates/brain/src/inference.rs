use std::path::PathBuf;

/// Resolves the model file path using the standard search order:
///
/// 1. `LODGE_MODEL_PATH` environment variable
/// 2. Alongside the running executable (`smollm2-360m-q4_k_m.gguf`)
/// 3. Platform data directory (`%LOCALAPPDATA%\lodge\model.gguf` / `~/.local/share/lodge/model.gguf`)
///
/// Returns `None` if none of the paths exist.
pub fn model_path() -> Option<PathBuf> {
    // 1. Env override
    if let Ok(p) = std::env::var("LODGE_MODEL_PATH") {
        let p = PathBuf::from(p);
        if p.exists() {
            return Some(p);
        }
    }

    // 2. Alongside the binary
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            for name in &["smollm2-360m-q4_k_m.gguf", "model.gguf"] {
                let candidate = dir.join(name);
                if candidate.exists() {
                    return Some(candidate);
                }
            }
        }
    }

    // 3. Platform data dir
    #[cfg(windows)]
    let data_dir = std::env::var("LOCALAPPDATA")
        .ok()
        .map(|d| PathBuf::from(d).join("lodge").join("model.gguf"));

    #[cfg(not(windows))]
    let data_dir = std::env::var("HOME")
        .ok()
        .map(|h| PathBuf::from(h).join(".local").join("share").join("lodge").join("model.gguf"));

    if let Some(p) = data_dir {
        if p.exists() {
            return Some(p);
        }
    }

    None
}

/// Wrapper around the llama.cpp inference backend.
///
/// Loads the GGUF model once at startup and keeps it resident.
/// Gated behind the `model` feature — without it the struct exists but
/// `load()` always returns `Ok(None)` from [`Brain::new`].
///
/// To enable full AI support:
/// ```text
/// cargo build --features lodge-brain/model
/// ```
///
/// The model file (`smollm2-360m-q4_k_m.gguf`) must be placed:
/// - Alongside the `lodge` binary, or
/// - At `%LOCALAPPDATA%\lodge\model.gguf` (Windows) /
///   `~/.local/share/lodge/model.gguf` (Unix), or
/// - At the path in the `LODGE_MODEL_PATH` environment variable.
pub struct InferenceEngine {
    #[cfg(feature = "model")]
    inner: ModelInner,
    #[cfg(not(feature = "model"))]
    _private: (),
}

#[cfg(feature = "model")]
struct ModelInner {
    backend: llama_cpp_2::llama_backend::LlamaBackend,
    model: llama_cpp_2::model::LlamaModel,
}

impl InferenceEngine {
    /// Load the model from `model_path`.
    ///
    /// Returns `Err` if the model file cannot be loaded.
    /// When the `model` feature is disabled, always returns `Err`.
    pub fn load(model_path: &std::path::Path) -> anyhow::Result<Self> {
        #[cfg(feature = "model")]
        {
            use anyhow::Context as _;
            use llama_cpp_2::{
                llama_backend::LlamaBackend,
                model::params::LlamaModelParams,
                model::LlamaModel,
            };

            let backend = LlamaBackend::init()
                .context("failed to initialise llama.cpp backend")?;
            let model_params = LlamaModelParams::default();
            let model = LlamaModel::load_from_file(&backend, model_path, &model_params)
                .with_context(|| format!("failed to load model from {:?}", model_path))?;
            Ok(Self { inner: ModelInner { backend, model } })
        }

        #[cfg(not(feature = "model"))]
        {
            let _ = model_path;
            Err(anyhow::anyhow!(
                "model feature not compiled in — rebuild with --features lodge-brain/model"
            ))
        }
    }

    /// Run inference on `prompt`, returning up to `max_new_tokens` tokens of output.
    ///
    /// The prompt should be pre-formatted with the chat template and system prompt.
    /// Returns raw model output (may include partial JSON — callers should parse).
    pub fn run(&self, prompt: &str, max_new_tokens: usize) -> anyhow::Result<String> {
        #[cfg(feature = "model")]
        {
            use anyhow::Context as _;
            use llama_cpp_2::{
                context::params::LlamaContextParams,
                llama_batch::LlamaBatch,
                model::{AddBos, Special},
                sampling::LlamaSampler,
            };
            use std::num::NonZeroU32;

            let ctx_params = LlamaContextParams::default()
                .with_n_ctx(NonZeroU32::new(2048));
            let mut ctx = self.inner.model
                .new_context(&self.inner.backend, ctx_params)
                .context("failed to create inference context")?;

            let tokens = self.inner.model
                .str_to_token(prompt, AddBos::Always)
                .context("failed to tokenize prompt")?;

            let n_input = tokens.len();
            let mut batch = LlamaBatch::new(n_input.max(1), 1);
            for (i, &tok) in tokens.iter().enumerate() {
                let is_last = i == n_input - 1;
                batch.add(tok, i as i32, &[0], is_last)
                    .context("failed to add token to batch")?;
            }
            ctx.decode(&mut batch).context("decode failed")?;

            let mut sampler = LlamaSampler::chain_simple([
                LlamaSampler::temp(0.1),
                LlamaSampler::greedy(),
            ]);

            let mut output = String::new();
            let mut n_cur = n_input;

            loop {
                let token = sampler.sample(&ctx, -1);
                sampler.accept(token);

                if self.inner.model.is_eog_token(token) || n_cur >= n_input + max_new_tokens {
                    break;
                }

                if let Ok(piece) = self.inner.model.token_to_str(token, Special::Tokenize) {
                    output.push_str(&piece);
                    // Stop at closing brace — we expect a single JSON object
                    if output.trim_end().ends_with('}') {
                        break;
                    }
                }

                batch.clear();
                batch.add(token, n_cur as i32, &[0], true)
                    .context("failed to add token to next batch")?;
                n_cur += 1;
                ctx.decode(&mut batch).context("decode failed")?;
            }

            Ok(output)
        }

        #[cfg(not(feature = "model"))]
        {
            let _ = (prompt, max_new_tokens);
            Err(anyhow::anyhow!("model feature not compiled in"))
        }
    }
}

/// Formats a prompt for SmolLM2-Instruct using its chat template.
///
/// SmolLM2 uses the ChatML template:
/// ```text
/// <|im_start|>system\n{system}\n<|im_end|>\n
/// <|im_start|>user\n{user}\n<|im_end|>\n
/// <|im_start|>assistant\n
/// ```
pub fn format_prompt(system: &str, context_prefix: &str, user_input: &str) -> String {
    let mut prompt = String::new();
    prompt.push_str("<|im_start|>system\n");
    prompt.push_str(system);
    prompt.push_str("\n<|im_end|>\n");
    if !context_prefix.is_empty() {
        prompt.push_str(context_prefix);
    }
    prompt.push_str("<|im_start|>user\n");
    prompt.push_str(user_input);
    prompt.push_str("\n<|im_end|>\n");
    prompt.push_str("<|im_start|>assistant\n");
    prompt
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn model_path_env_override() {
        // Pointing to a non-existent path should not return it
        // (model_path only returns paths that exist)
        unsafe { std::env::set_var("LODGE_MODEL_PATH", "/nonexistent/model.gguf") };
        let p = model_path();
        unsafe { std::env::remove_var("LODGE_MODEL_PATH") };
        // The path doesn't exist so model_path should not return it
        assert!(p.is_none() || p.unwrap().exists());
    }

    #[test]
    fn format_prompt_contains_sections() {
        let p = format_prompt("system text", "", "user input");
        assert!(p.contains("<|im_start|>system"));
        assert!(p.contains("system text"));
        assert!(p.contains("<|im_start|>user"));
        assert!(p.contains("user input"));
        assert!(p.contains("<|im_start|>assistant"));
    }

    #[test]
    fn format_prompt_includes_context() {
        let p = format_prompt("sys", "prior context\n", "question");
        assert!(p.contains("prior context"));
        assert!(p.contains("question"));
    }

    #[test]
    fn load_fails_gracefully_without_model_file() {
        let result = InferenceEngine::load(std::path::Path::new("/nonexistent/model.gguf"));
        assert!(result.is_err());
    }
}
