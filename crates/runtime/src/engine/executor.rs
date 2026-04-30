use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use lodge_shared::placement::{PlacementEntry, PlacementPlan};

/// The state of a single installation step.
#[derive(Debug, Clone, PartialEq, Eq)]
#[allow(dead_code)]
pub enum StepState {
    Pending,
    InProgress,
    Done,
    Failed(String),
    Warning(String),
}

/// Progress event emitted by [`execute`] for each step.
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct StepEvent {
    /// Zero-based index into the step list.
    pub index: usize,
    pub state: StepState,
    /// Short description of the destination (shown in the sequence screen).
    pub detail: String,
}

/// Executes a resolved [`PlacementPlan`], emitting [`StepEvent`]s via `on_step`.
///
/// Steps in emission order:
/// 1. Pre-install hook (if any)
/// 2. One event per [`PlacementEntry`] (InProgress → Done/Failed)
/// 3. Post-install hook (if any)
///
/// On first `Failed` step the error is reported but execution continues —
/// Lodge records partial placements in the receipt so they can be reversed.
#[allow(dead_code)]
pub fn execute(
    plan: &PlacementPlan,
    pkg_root: &Path,
    on_step: &mut dyn FnMut(StepEvent),
) -> Result<Vec<String>> {
    let mut hooks_run: Vec<String> = Vec::new();
    let mut step_idx = 0;

    // Pre-install hook
    if let Some(hook) = plan.hooks_order.first() {
        if hook.contains("pre") {
            on_step(StepEvent {
                index: step_idx,
                state: StepState::InProgress,
                detail: hook.clone(),
            });
            match run_hook(hook, pkg_root) {
                Ok(()) => {
                    hooks_run.push(hook.clone());
                    on_step(StepEvent {
                        index: step_idx,
                        state: StepState::Done,
                        detail: hook.clone(),
                    });
                }
                Err(e) => {
                    on_step(StepEvent {
                        index: step_idx,
                        state: StepState::Failed(e.to_string()),
                        detail: hook.clone(),
                    });
                }
            }
            step_idx += 1;
        }
    }

    // File placements
    for entry in &plan.entries {
        let detail = entry.destination.to_string_lossy().into_owned();
        on_step(StepEvent {
            index: step_idx,
            state: StepState::InProgress,
            detail: detail.clone(),
        });

        match place_file(entry) {
            Ok(_) => {
                on_step(StepEvent {
                    index: step_idx,
                    state: StepState::Done,
                    detail,
                });
            }
            Err(e) => {
                on_step(StepEvent {
                    index: step_idx,
                    state: StepState::Failed(e.to_string()),
                    detail,
                });
            }
        }
        step_idx += 1;
    }

    // Post-install hook
    if let Some(hook) = plan.hooks_order.last() {
        if hook.contains("post") {
            on_step(StepEvent {
                index: step_idx,
                state: StepState::InProgress,
                detail: hook.clone(),
            });
            match run_hook(hook, pkg_root) {
                Ok(()) => {
                    hooks_run.push(hook.clone());
                    on_step(StepEvent {
                        index: step_idx,
                        state: StepState::Done,
                        detail: hook.clone(),
                    });
                }
                Err(e) => {
                    on_step(StepEvent {
                        index: step_idx,
                        state: StepState::Failed(e.to_string()),
                        detail: hook.clone(),
                    });
                }
            }
        }
    }

    Ok(hooks_run)
}

/// Copies a single [`PlacementEntry`] to its destination, creating parent directories.
pub fn place_file(entry: &PlacementEntry) -> Result<u64> {
    let dest = effective_destination(entry);

    if let Some(parent) = dest.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("couldn't create directory {:?}", parent))?;
    }

    let bytes = std::fs::copy(&entry.source, &dest)
        .with_context(|| format!("couldn't copy {:?} → {:?}", entry.source, dest))?;

    Ok(bytes)
}

/// Returns the effective destination path, applying any rename declared in the entry.
pub fn effective_destination(entry: &PlacementEntry) -> PathBuf {
    match &entry.rename {
        Some(name) => entry
            .destination
            .parent()
            .unwrap_or(&entry.destination)
            .join(name),
        None => entry.destination.clone(),
    }
}

/// Runs a lifecycle hook script relative to the package root.
///
/// Dispatches to PowerShell on Windows, sh on Unix.
pub fn run_hook(script: &str, pkg_root: &Path) -> Result<()> {
    let script_path = pkg_root.join(script);
    anyhow::ensure!(
        script_path.exists(),
        "hook script not found: {:?}",
        script_path
    );

    #[cfg(windows)]
    let status = std::process::Command::new("powershell")
        .args(["-ExecutionPolicy", "Bypass", "-File"])
        .arg(&script_path)
        .current_dir(pkg_root)
        .status()
        .with_context(|| format!("couldn't launch hook {:?}", script_path))?;

    #[cfg(not(windows))]
    let status = std::process::Command::new("sh")
        .arg(&script_path)
        .current_dir(pkg_root)
        .status()
        .with_context(|| format!("couldn't launch hook {:?}", script_path))?;

    anyhow::ensure!(status.success(), "hook {:?} exited with {}", script, status);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use lodge_shared::placement::{PlacementEntry, PlacementPlan, RegistrationEffects};
    use std::fs;
    use tempfile::tempdir;

    fn entry(src: &Path, dst: &Path) -> PlacementEntry {
        PlacementEntry {
            source: src.to_path_buf(),
            destination: dst.to_path_buf(),
            rename: None,
        }
    }

    fn plan_with(entries: Vec<PlacementEntry>) -> PlacementPlan {
        PlacementPlan {
            entries,
            registrations: RegistrationEffects::default(),
            hooks_order: vec![],
            requires_elevation: false,
        }
    }

    #[test]
    fn place_file_copies_to_destination() {
        let src_dir = tempdir().unwrap();
        let dst_dir = tempdir().unwrap();

        let src = src_dir.path().join("tool.exe");
        let dst = dst_dir.path().join("programs").join("tool.exe");
        fs::write(&src, b"fake binary").unwrap();

        let e = entry(&src, &dst);
        place_file(&e).unwrap();

        assert!(dst.exists());
        assert_eq!(fs::read(&dst).unwrap(), b"fake binary");
    }

    #[test]
    fn place_file_creates_parent_directories() {
        let src_dir = tempdir().unwrap();
        let dst_dir = tempdir().unwrap();

        let src = src_dir.path().join("x");
        let dst = dst_dir.path().join("a").join("b").join("c").join("x");
        fs::write(&src, b"data").unwrap();

        place_file(&entry(&src, &dst)).unwrap();
        assert!(dst.exists());
    }

    #[test]
    fn effective_destination_applies_rename() {
        let e = PlacementEntry {
            source: PathBuf::from("src/mt.exe"),
            destination: PathBuf::from("C:/Programs/mytool/mt.exe"),
            rename: Some("lodge.exe".into()),
        };
        let dest = effective_destination(&e);
        assert_eq!(dest.file_name().unwrap(), "lodge.exe");
    }

    #[test]
    fn effective_destination_without_rename_unchanged() {
        let e = PlacementEntry {
            source: PathBuf::from("src/tool.exe"),
            destination: PathBuf::from("C:/Programs/tool.exe"),
            rename: None,
        };
        assert_eq!(effective_destination(&e), e.destination);
    }

    #[test]
    fn execute_emits_in_progress_then_done() {
        let src_dir = tempdir().unwrap();
        let dst_dir = tempdir().unwrap();
        let src = src_dir.path().join("f");
        let dst = dst_dir.path().join("f");
        fs::write(&src, b"").unwrap();

        let plan = plan_with(vec![entry(&src, &dst)]);
        let mut events: Vec<(StepState, String)> = Vec::new();

        execute(&plan, src_dir.path(), &mut |ev| {
            events.push((ev.state.clone(), ev.detail.clone()));
        })
        .unwrap();

        assert!(events.iter().any(|(s, _)| *s == StepState::InProgress));
        assert!(events.iter().any(|(s, _)| *s == StepState::Done));
    }
}
