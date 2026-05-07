//! Clean Cabin — Lodge extension for interactive file system cleanup.
//!
//! Dispatched by Lodge when the user types `!clean`. Can also be run as a
//! standalone binary.

mod cabin_trash;
mod config;
mod palette;
mod report;
mod scanner;

use std::io::{self, Write};
use std::path::PathBuf;
use std::thread;
use std::time::Duration;

use anyhow::{Context, Result};
use crossterm::event::{self, Event, KeyCode, KeyModifiers};
use crossterm::execute;
use crossterm::terminal::{
    disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen,
};
use ratatui::backend::CrosstermBackend;
use ratatui::Terminal;

use cabin_trash::staging;
use config::Config;
use report::selector::{FlaggedFile, SelectionState};
use report::tiers::Tier;
use scanner::heuristics::TierHint;

// ── Entry point ───────────────────────────────────────────────────────────────

fn main() {
    if let Err(e) = run() {
        eprintln!("clean-cabin: {e}");
        std::process::exit(1);
    }
}

fn run() -> Result<()> {
    let raw_args: Vec<String> = std::env::args().skip(1).collect();

    // Strip Lodge-injected control flags before subcommand dispatch.
    let lodge_launched = raw_args.iter().any(|a| a == "--lodge");
    let lodge_path: Option<PathBuf> = {
        let mut p = None;
        let mut iter = raw_args.iter();
        while let Some(a) = iter.next() {
            if a == "--path" {
                p = iter.next().map(PathBuf::from);
                break;
            }
        }
        p
    };
    let args: Vec<String> = {
        let mut out = Vec::new();
        let mut iter = raw_args.iter();
        while let Some(a) = iter.next() {
            if a == "--lodge" { continue; }
            if a == "--path" { iter.next(); continue; } // skip value too
            out.push(a.clone());
        }
        out
    };

    match args.as_slice() {
        [] | [_] if args.first().map(|a| a == "clean").unwrap_or(true) => {
            cmd_scan(lodge_path, lodge_launched)
        }
        [sub] if sub == "recover" => cmd_recover(false),
        [sub, flag] if sub == "recover" && flag == "--all" => cmd_recover(true),
        [sub] if sub == "purge" => cmd_purge(None),
        [sub, flag, date] if sub == "purge" && flag == "--before" => {
            let cutoff = chrono::NaiveDate::parse_from_str(date, "%Y-%m-%d")
                .with_context(|| format!("invalid date '{date}' — expected YYYY-MM-DD"))?;
            cmd_purge(Some(cutoff))
        }
        [sub] if sub == "config" => cmd_config_show(),
        [sub, key, value] if sub == "config" && key.as_str() == "set" => {
            Err(anyhow::anyhow!("usage: clean-cabin config set <key> <value>"))
        }
        [sub, cmd, key, value] if sub == "config" && cmd.as_str() == "set" => {
            cmd_config_set(key, value)
        }
        _ => {
            eprintln!(
                "usage: clean-cabin [clean | recover [--all] | purge [--before YYYY-MM-DD] | config [set <key> <value>]]"
            );
            std::process::exit(1);
        }
    }
}

// ── Subcommands ───────────────────────────────────────────────────────────────

/// Full scan → interactive report → staging flow.
fn cmd_scan(scan_path: Option<PathBuf>, _lodge_launched: bool) -> Result<()> {
    let config = Config::load();

    // Use Lodge-provided path, or prompt the user interactively.
    let dirs = if let Some(p) = scan_path {
        if p.exists() {
            vec![p]
        } else {
            anyhow::bail!("path does not exist: {}", p.display());
        }
    } else {
        let d = prompt_directories()?;
        if d.is_empty() {
            println!("nothing to scan.");
            return Ok(());
        }
        d
    };

    // Auto-purge old sessions silently.
    staging::auto_purge(config.retention_days);

    // Walk and score.
    println!("scanning...");
    let entries = scanner::walk(&dirs, &config.scan_exclude);
    let total = entries.len();
    println!("  {total} files found — scoring...");

    let guarded = scanner::receipt_guard::load_receipt_paths();
    let mut hashes = std::collections::HashMap::new();

    // Heuristic scoring.
    let mut scored: Vec<(scanner::walker::FileEntry, scanner::heuristics::HeuristicScore)> = entries
        .into_iter()
        .map(|entry| {
            let mut score = scanner::heuristics::score(&entry, &config, &mut hashes);
            score.is_receipt_guarded = scanner::receipt_guard::is_guarded(&entry.path, &guarded);
            (entry, score)
        })
        .filter(|(_, s)| s.tier_hint != TierHint::Keep)
        .collect();

    // Optional AI scoring.
    let no_ai = config.ai_mode == config::AiMode::None;
    let ai_scores: Vec<scanner::ai_scorer::AiScore> = if !no_ai {
        print!("  running AI scorer...");
        io::stdout().flush().unwrap_or_default();
        let entries_ref: Vec<&scanner::walker::FileEntry> =
            scored.iter().map(|(e, _)| e).collect();
        let ai = scanner::ai_scorer::score_batch(&entries_ref, &config);
        println!(" done.");
        ai
    } else {
        Vec::new()
    };

    // Build flagged file list.
    let flagged: Vec<FlaggedFile> = scored
        .drain(..)
        .enumerate()
        .map(|(i, (entry, h))| {
            let ai = ai_scores.get(i);
            let tier = report::tiers::classify(&h, ai);
            let reason = if let Some(a) = ai {
                if !a.reason.is_empty() && !h.reason.is_empty() {
                    format!("{} — {}", h.reason, a.reason)
                } else if !a.reason.is_empty() {
                    a.reason.clone()
                } else {
                    h.reason.clone()
                }
            } else {
                h.reason.clone()
            };

            FlaggedFile {
                entry,
                tier,
                reason,
                guarded: h.is_receipt_guarded,
                selected: false,
            }
        })
        .filter(|f| f.tier != Tier::YouDecide || !no_ai)
        .collect();

    if flagged.is_empty() {
        println!("nothing to clean up. the cabin is already tidy.");
        return Ok(());
    }

    let mut state = SelectionState::new(flagged);

    // Enter ratatui TUI.
    let mut terminal = enter_tui()?;

    let result = run_report_loop(&mut terminal, &mut state, no_ai);

    leave_tui(&mut terminal)?;

    match result? {
        ReportOutcome::Quit => {
            println!("nothing moved.");
        }
        ReportOutcome::Proceed => {
            let selected: Vec<&FlaggedFile> = state.selected_files();
            if selected.is_empty() {
                println!("nothing selected.");
                return Ok(());
            }

            // Confirmation screen.
            let session_id = staging::new_session_id();
            let n = selected.len();
            let size = state.selected_size();
            let root = staging::staging_root();
            let retention = config.retention_days;

            let confirmed = run_confirmation_screen(n, size, &session_id, retention, &root)?;
            if !confirmed {
                println!("nothing moved.");
                return Ok(());
            }

            // Stage files with live progress.
            run_staging_progress(&selected, &session_id, &config)?;
        }
    }

    Ok(())
}

enum ReportOutcome {
    Quit,
    Proceed,
}

fn run_report_loop(
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    state: &mut SelectionState,
    no_ai: bool,
) -> Result<ReportOutcome> {
    loop {
        terminal.draw(|f| report::renderer::render(state, no_ai, f))?;

        if event::poll(Duration::from_millis(50))? {
            if let Event::Key(key) = event::read()? {
                match (key.code, key.modifiers) {
                    (KeyCode::Char('q') | KeyCode::Esc, _) => {
                        return Ok(ReportOutcome::Quit);
                    }
                    (KeyCode::Char('c'), KeyModifiers::CONTROL) => {
                        return Ok(ReportOutcome::Quit);
                    }
                    (KeyCode::Enter, _) if !state.selected_files().is_empty() => {
                        return Ok(ReportOutcome::Proceed);
                    }
                    (KeyCode::Char(' '), _) => state.toggle_current(),
                    (KeyCode::Up, _) => state.move_up(),
                    (KeyCode::Down, _) => state.move_down(),
                    (KeyCode::Tab, _) => state.cycle_tier(),
                    (KeyCode::Right, _) => {
                        state.you_decide_expanded = !state.you_decide_expanded;
                    }
                    (KeyCode::Char('a') | KeyCode::Char('A'), _) => {
                        state.select_all_in_tier(state.tier_focus);
                    }
                    (KeyCode::Char('n') | KeyCode::Char('N'), _) => {
                        state.deselect_all_in_tier(state.tier_focus);
                    }
                    _ => {}
                }
            }
        }
    }
}

fn run_confirmation_screen(
    n: usize,
    size: u64,
    session_id: &str,
    retention_days: u32,
    root: &std::path::Path,
) -> Result<bool> {
    let mut terminal = enter_tui()?;

    let result = loop {
        terminal.draw(|f| {
            report::renderer::render_confirmation(n, size, session_id, retention_days, root, f)
        })?;

        if event::poll(Duration::from_millis(50))? {
            if let Event::Key(key) = event::read()? {
                match key.code {
                    KeyCode::Enter => break true,
                    KeyCode::Esc => break false,
                    KeyCode::Char('c') if key.modifiers == KeyModifiers::CONTROL => break false,
                    _ => {}
                }
            }
        }
    };

    leave_tui(&mut terminal)?;
    Ok(result)
}

fn run_staging_progress(
    selected: &[&FlaggedFile],
    session_id: &str,
    config: &Config,
) -> Result<()> {
    let manifest = staging::stage_files(selected, session_id)
        .context("staging failed")?;

    let n = manifest.files.len();
    let total_size: u64 = manifest.files.iter().map(|f| f.size).sum();
    let expiry = staging::expiry_date(session_id, config.retention_days)
        .map(|d| d.format("%Y-%m-%d").to_string())
        .unwrap_or_else(|| "unknown".into());

    // Simple stdout summary (TUI already left).
    println!();
    for sf in &manifest.files {
        let name = sf
            .original_path
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| sf.staged_name.clone());
        println!("  ✔  {name}  → staging");
    }

    let size_str = fmt_size(total_size);
    println!();
    println!("  {n} files moved to staging. {size_str} freed.");
    println!(
        "  they'll be permanently removed on {expiry} unless recovered."
    );
    println!("  recover with:  clean-cabin recover");
    println!();

    thread::sleep(Duration::from_secs(3));
    Ok(())
}

/// Interactive recovery: list sessions and let the user choose one.
fn cmd_recover(recover_all: bool) -> Result<()> {
    let sessions = staging::list_sessions();

    if sessions.is_empty() {
        println!("no staged sessions found.");
        return Ok(());
    }

    if recover_all {
        // Recover the most recent session.
        let (session_id, manifest) = sessions.last().context("no sessions")?;
        let n = staging::recover_session(session_id)?;
        println!(
            "  recovered {n} / {} files from session {session_id}.",
            manifest.files.len()
        );
        return Ok(());
    }

    println!("staged sessions:\n");
    for (i, (session_id, manifest)) in sessions.iter().enumerate() {
        let total_size: u64 = manifest.files.iter().map(|f| f.size).sum();
        println!(
            "  [{i}]  {session_id}  ({} files · {})",
            manifest.files.len(),
            fmt_size(total_size),
        );
    }

    print!("\nrecover which session? (index or Enter to cancel): ");
    io::stdout().flush().unwrap_or_default();

    let mut input = String::new();
    io::stdin().read_line(&mut input).unwrap_or_default();
    let input = input.trim();

    if input.is_empty() {
        println!("cancelled.");
        return Ok(());
    }

    let idx: usize = input
        .parse()
        .with_context(|| format!("'{input}' is not a valid index"))?;

    let (session_id, manifest) = sessions.get(idx).context("index out of range")?;
    let n = staging::recover_session(session_id)?;
    println!(
        "  recovered {n} / {} files from session {session_id}.",
        manifest.files.len()
    );

    Ok(())
}

/// Purge staged files, optionally restricted to sessions before a cutoff date.
fn cmd_purge(cutoff: Option<chrono::NaiveDate>) -> Result<()> {
    match cutoff {
        Some(date) => {
            let (count, freed) = staging::purge_before(date)?;
            println!(
                "  purged {count} sessions before {date}. {} freed.",
                fmt_size(freed)
            );
        }
        None => {
            print!("purge all staged sessions? this cannot be undone. [y/N]: ");
            io::stdout().flush().unwrap_or_default();
            let mut input = String::new();
            io::stdin().read_line(&mut input).unwrap_or_default();
            if input.trim().eq_ignore_ascii_case("y") {
                let sessions = staging::list_sessions();
                let mut total_freed = 0u64;
                for (session_id, _) in &sessions {
                    total_freed += staging::purge_session(session_id).unwrap_or(0);
                }
                println!(
                    "  purged {} sessions. {} freed.",
                    sessions.len(),
                    fmt_size(total_freed)
                );
            } else {
                println!("  cancelled.");
            }
        }
    }
    Ok(())
}

fn cmd_config_show() -> Result<()> {
    let config = Config::load();
    println!("{}", serde_json::to_string_pretty(&config)?);
    Ok(())
}

fn cmd_config_set(key: &str, value: &str) -> Result<()> {
    let mut config = Config::load();
    config.set(key, value)?;
    config.save()?;
    println!("  {key} = {value}");
    Ok(())
}

// ── TUI helpers ───────────────────────────────────────────────────────────────

fn enter_tui() -> Result<Terminal<CrosstermBackend<io::Stdout>>> {
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(stdout);
    let terminal = Terminal::new(backend)?;
    Ok(terminal)
}

fn leave_tui(terminal: &mut Terminal<CrosstermBackend<io::Stdout>>) -> Result<()> {
    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
    terminal.show_cursor()?;
    Ok(())
}

// ── Utilities ─────────────────────────────────────────────────────────────────

fn prompt_directories() -> Result<Vec<PathBuf>> {
    print!("directory to scan (Enter for home): ");
    io::stdout().flush().unwrap_or_default();

    let mut input = String::new();
    io::stdin().read_line(&mut input).unwrap_or_default();
    let input = input.trim();

    if input.is_empty() {
        // Default to home directory.
        let home = home_dir().context("couldn't determine home directory")?;
        return Ok(vec![home]);
    }

    let paths: Vec<PathBuf> = input
        .split(';')
        .map(|s| PathBuf::from(s.trim()))
        .filter(|p| p.exists())
        .collect();

    if paths.is_empty() {
        anyhow::bail!("none of the specified paths exist");
    }

    Ok(paths)
}

fn home_dir() -> Option<PathBuf> {
    #[cfg(windows)]
    {
        std::env::var_os("USERPROFILE").map(PathBuf::from)
    }
    #[cfg(not(windows))]
    {
        std::env::var_os("HOME").map(PathBuf::from)
    }
}

fn fmt_size(bytes: u64) -> String {
    if bytes == 0 {
        return "0 B".into();
    }
    let mb = bytes / (1024 * 1024);
    if mb >= 1 {
        format!("{mb} MB")
    } else {
        let kb = bytes / 1024;
        if kb >= 1 {
            format!("{kb} KB")
        } else {
            format!("{bytes} B")
        }
    }
}
