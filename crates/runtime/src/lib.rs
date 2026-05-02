/// Public surface for integration tests and future embedding.
pub mod engine;
pub mod shim;

/// Runtime version, re-exported so engine modules can reference it without
/// depending on `main.rs`.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");
