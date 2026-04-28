#![allow(dead_code)]

use std::path::Path;

/// Registers a command shim for an installed CLI tool.
///
/// Windows: writes a `.cmd` file to `%LOCALAPPDATA%\Programs\lodge\shims\`.
/// Unix:    creates a symlink in `~/.local/bin/`.
pub fn register(_command_name: &str, _target: &Path) -> anyhow::Result<()> {
    todo!()
}

/// Removes the shim for a command name.
pub fn unregister(_command_name: &str) -> anyhow::Result<()> {
    todo!()
}

/// Updates an existing shim to point at a new target (for version switching).
pub fn update(_command_name: &str, _new_target: &Path) -> anyhow::Result<()> {
    unregister(_command_name)?;
    register(_command_name, _new_target)
}
