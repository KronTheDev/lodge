/// Wrapper around the llama.cpp backend.
///
/// Loads the model once at startup and keeps it resident for the session.
/// The llama.cpp binding crate is TBD — evaluate `llama-cpp-2` vs `llama-cpp-rs`
/// vs direct bindgen before implementing this module.
pub struct InferenceEngine {
    // model: LlamaModel,
}

impl InferenceEngine {
    /// Load the model from `model_path`. Fails with a helpful message if not found.
    pub fn load(_model_path: &std::path::Path) -> anyhow::Result<Self> {
        todo!("llama.cpp binding not yet selected — implement after evaluating crate options")
    }

    /// Run inference and return raw model output as a string.
    pub fn run(&self, _prompt: &str) -> anyhow::Result<String> {
        todo!()
    }
}
