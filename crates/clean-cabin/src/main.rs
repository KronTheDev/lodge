//! Clean Cabin — Lodge extension for interactive file system cleanup.
//!
//! Dispatched by Lodge when the user types `!clean`. Can also be run as a
//! standalone binary.

mod cabin_trash;
mod config;
mod palette;
mod report;
mod scanner;

use std::io;
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
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
        // Attempt to restore terminal before printing error.
        let _ = disable_raw_mode();
        let _ = execute!(io::stdout(), LeaveAlternateScreen);
        eprintln!("clean-cabin: {e}");
        std::process::exit(1);
    }
}

fn run() -> Result<()> {
    let raw_args: Vec<String> = std::env::args().skip(1).collect();

    // Strip Lodge-injected control flags before subcommand dispatch.
    let _lodge_launched = raw_args.iter().any(|a| a == "--lodge");
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
            if a == "--lodge" {
                continue;
            }
            if a == "--path" {
                iter.next(); // skip value
                continue;
            }
            out.push(a.clone());
        }
        out
    };

    match args.as_slice() {
        [] | [_] if args.first().map(|a| a == "clean").unwrap_or(true) => {
            cmd_scan(lodge_path)
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
fn cmd_scan(scan_path: Option<PathBuf>) -> Result<()> {
    let config = Config::load();

    // Auto-purge old sessions silently.
    staging::auto_purge(config.retention_days);

    // Single enter_tui() / leave_tui() pair for the whole flow.
    let mut terminal = enter_tui()?;

    // ── Step 2: Directory prompt ──────────────────────────────────────────────
    let result = run_dir_prompt(&mut terminal, scan_path.clone());
    let (dirs, exclusions) = match result {
        None => {
            leave_tui(&mut terminal)?;
            return Ok(());
        }
        Some(pair) => pair,
    };

    // ── Step 3: Scan progress ─────────────────────────────────────────────────
    let mut combined_exclude = config.scan_exclude.clone();
    combined_exclude.extend(exclusions);

    let entries = run_scan_progress(&mut terminal, &dirs, &combined_exclude)?;
    let total = entries.len();
    let _ = total;

    // ── Step 4: Score + AI ────────────────────────────────────────────────────
    {
        // Show "thinking..." while scoring.
        let dirs_clone = dirs.clone();
        terminal.draw(|f| {
            report::renderer::render_scan_progress(
                &dirs_label(&dirs_clone),
                entries.len(),
                0,
                0,
                f,
            );
        })?;
    }

    let guarded = scanner::receipt_guard::load_receipt_paths();
    let mut hashes = std::collections::HashMap::new();

    let mut scored: Vec<(scanner::walker::FileEntry, scanner::heuristics::HeuristicScore)> =
        entries
            .into_iter()
            .map(|entry| {
                let mut score = scanner::heuristics::score(&entry, &config, &mut hashes);
                score.is_receipt_guarded =
                    scanner::receipt_guard::is_guarded(&entry.path, &guarded);
                (entry, score)
            })
            .filter(|(_, s)| s.tier_hint != TierHint::Keep)
            .collect();

    let no_ai = config.ai_mode == config::AiMode::None;
    let ai_scores: Vec<scanner::ai_scorer::AiScore> = if !no_ai {
        let entries_ref: Vec<&scanner::walker::FileEntry> =
            scored.iter().map(|(e, _)| e).collect();
        scanner::ai_scorer::score_batch(&entries_ref, &config)
    } else {
        Vec::new()
    };

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

    // ── Step 5 / 6: Empty result ──────────────────────────────────────────────
    if flagged.is_empty() {
        run_message_screen(
            &mut terminal,
            "nothing to clean up. the cabin is already tidy.",
        )?;
        leave_tui(&mut terminal)?;
        return Ok(());
    }

    let mut state = SelectionState::new(flagged);

    // ── Step 5: Interactive report ────────────────────────────────────────────
    let outcome = run_report_loop(&mut terminal, &mut state, no_ai, &dirs)?;

    if let ReportOutcome::Quit = outcome {
        leave_tui(&mut terminal)?;
        return Ok(());
    }

    let selected: Vec<&FlaggedFile> = state.selected_files();
    if selected.is_empty() {
        leave_tui(&mut terminal)?;
        return Ok(());
    }

    // ── Step 6: Confirmation ──────────────────────────────────────────────────
    let session_id = staging::new_session_id();
    let n = selected.len();
    let size = state.selected_size();
    let root = staging::staging_root();

    let confirmed = run_confirmation_screen(
        &mut terminal,
        n,
        size,
        &session_id,
        config.retention_days,
        &root,
    )?;

    if !confirmed {
        leave_tui(&mut terminal)?;
        return Ok(());
    }

    // ── Step 7: Staging sequence ──────────────────────────────────────────────
    let (session_dir, manifest) = staging::init_session(&selected, &session_id)
        .context("couldn't initialise staging session")?;

    run_staging_sequence(&mut terminal, &manifest, &session_dir, &config)?;

    leave_tui(&mut terminal)?;
    Ok(())
}

enum ReportOutcome {
    Quit,
    Proceed,
}

// ── TUI screens ───────────────────────────────────────────────────────────────

/// Step 2: Directory prompt. Returns `None` if user cancelled.
fn run_dir_prompt(
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    initial_path: Option<PathBuf>,
) -> Option<(Vec<PathBuf>, Vec<String>)> {
    let mut confirmed_dirs: Vec<PathBuf> = Vec::new();
    let mut exclusions: Vec<String> = Vec::new();
    let mut input = String::new();
    let mut exclusion_mode = false;

    // Pre-fill with initial_path if provided.
    if let Some(ref p) = initial_path {
        input = p.to_string_lossy().to_string();
    }

    loop {
        let _ = terminal.draw(|f| {
            report::renderer::render_dir_prompt(
                &confirmed_dirs,
                &exclusions,
                &input,
                exclusion_mode,
                f,
            );
        });

        let Ok(true) = event::poll(Duration::from_millis(50)) else {
            continue;
        };

        let Ok(Event::Key(key)) = event::read() else {
            continue;
        };

        match (key.code, key.modifiers) {
            (KeyCode::Esc, _)
            | (KeyCode::Char('c'), KeyModifiers::CONTROL) => {
                return None;
            }
            (KeyCode::Enter, _) => {
                if !input.is_empty() {
                    let p = PathBuf::from(input.trim());
                    if exclusion_mode {
                        exclusions.push(input.trim().to_string());
                    } else {
                        confirmed_dirs.push(p);
                    }
                    input.clear();
                    exclusion_mode = false;
                } else {
                    // Empty input — proceed with what we have.
                    if confirmed_dirs.is_empty() {
                        // Default to home directory.
                        if let Some(home) = home_dir() {
                            confirmed_dirs.push(home);
                        } else {
                            continue;
                        }
                    }
                    // Filter to only existing dirs.
                    let valid: Vec<PathBuf> = confirmed_dirs
                        .into_iter()
                        .filter(|p| p.exists())
                        .collect();
                    if valid.is_empty() {
                        return None;
                    }
                    return Some((valid, exclusions));
                }
            }
            (KeyCode::Tab, _) if !input.is_empty() => {
                // Add current path to confirmed list, clear for another.
                let p = PathBuf::from(input.trim());
                if exclusion_mode {
                    exclusions.push(input.trim().to_string());
                    exclusion_mode = false;
                } else {
                    confirmed_dirs.push(p);
                }
                input.clear();
            }
            (KeyCode::Tab, _) => {}
            (KeyCode::Char('e') | KeyCode::Char('E'), KeyModifiers::NONE)
            | (KeyCode::Char('e') | KeyCode::Char('E'), KeyModifiers::SHIFT) => {
                if input.is_empty() {
                    exclusion_mode = true;
                } else {
                    input.push(if key.code == KeyCode::Char('e') { 'e' } else { 'E' });
                }
            }
            (KeyCode::Backspace, _) => {
                input.pop();
            }
            (KeyCode::Char(c), _) => {
                input.push(c);
            }
            _ => {}
        }
    }
}

/// Step 3: Scan progress — walks in a background thread, polls for updates.
fn run_scan_progress(
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    dirs: &[PathBuf],
    exclude: &[String],
) -> Result<Vec<scanner::walker::FileEntry>> {
    let counter = Arc::new(AtomicUsize::new(0));
    let counter_clone = counter.clone();

    let dirs_clone = dirs.to_vec();
    let exclude_clone = exclude.to_vec();
    let label = dirs_label(dirs);

    // Spawn walk on background thread.
    let handle = std::thread::spawn(move || {
        scanner::walker::walk_with_progress(
            &dirs_clone,
            &exclude_clone,
            counter_clone,
            |_| {},
        )
    });

    let mut spinner_frame: u8 = 0;
    let mut frame_tick: u32 = 0;

    loop {
        let count = counter.load(Ordering::Relaxed);

        terminal.draw(|f| {
            report::renderer::render_scan_progress(&label, count, 0, spinner_frame, f);
        })?;

        if handle.is_finished() {
            break;
        }

        std::thread::sleep(Duration::from_millis(50));
        frame_tick += 1;
        if frame_tick.is_multiple_of(4) {
            spinner_frame = spinner_frame.wrapping_add(1);
        }

        // Check for Ctrl+C / Esc while scanning.
        if event::poll(Duration::from_millis(0))? {
            if let Event::Key(key) = event::read()? {
                if matches!(key.code, KeyCode::Esc)
                    || (key.code == KeyCode::Char('c')
                        && key.modifiers == KeyModifiers::CONTROL)
                {
                    // We can't cancel the thread easily; just return empty.
                    return Ok(Vec::new());
                }
            }
        }
    }

    let entries = handle.join().unwrap_or_default();
    Ok(entries)
}

/// Step 5: Interactive report loop.
fn run_report_loop(
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    state: &mut SelectionState,
    no_ai: bool,
    dirs: &[PathBuf],
) -> Result<ReportOutcome> {
    loop {
        terminal.draw(|f| report::renderer::render(state, no_ai, dirs, f))?;

        if !event::poll(Duration::from_millis(50))? {
            continue;
        }

        let Event::Key(key) = event::read()? else {
            continue;
        };

        match (key.code, key.modifiers) {
            (KeyCode::Char('q') | KeyCode::Char('Q'), _) | (KeyCode::Esc, _) => {
                // If detail panel is open, Esc closes it instead of quitting.
                if state.detail_file.is_some() {
                    state.detail_file = None;
                } else {
                    return Ok(ReportOutcome::Quit);
                }
            }
            (KeyCode::Char('c'), KeyModifiers::CONTROL) => {
                return Ok(ReportOutcome::Quit);
            }
            (KeyCode::Enter, _) => {
                if state.detail_file.is_some() {
                    // Close detail panel.
                    state.detail_file = None;
                } else if !state.selected_files().is_empty() {
                    return Ok(ReportOutcome::Proceed);
                } else {
                    // No files selected — open detail for cursor file.
                    if state.visible_count() > 0 {
                        state.detail_file = Some(state.cursor);
                    }
                }
            }
            (KeyCode::Char(' '), _) if state.detail_file.is_none() => {
                state.toggle_current();
            }
            (KeyCode::Up, _) if state.detail_file.is_none() => {
                state.move_up();
            }
            (KeyCode::Down, _) if state.detail_file.is_none() => {
                state.move_down();
            }
            (KeyCode::Tab, _) if state.detail_file.is_none() => {
                state.cycle_tier();
            }
            (KeyCode::Right, _) if state.detail_file.is_none() => {
                state.you_decide_expanded = !state.you_decide_expanded;
            }
            (KeyCode::Char('a') | KeyCode::Char('A'), _) if state.detail_file.is_none() => {
                state.select_all_in_tier(state.tier_focus);
            }
            (KeyCode::Char('n') | KeyCode::Char('N'), _) if state.detail_file.is_none() => {
                state.deselect_all_in_tier(state.tier_focus);
            }
            _ => {}
        }
    }
}

/// Step 6: Confirmation screen.
fn run_confirmation_screen(
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    n: usize,
    size: u64,
    session_id: &str,
    retention_days: u32,
    root: &std::path::Path,
) -> Result<bool> {
    loop {
        terminal.draw(|f| {
            report::renderer::render_confirmation(n, size, session_id, retention_days, root, f);
        })?;

        if !event::poll(Duration::from_millis(50))? {
            continue;
        }

        if let Event::Key(key) = event::read()? {
            match key.code {
                KeyCode::Enter => return Ok(true),
                KeyCode::Esc => return Ok(false),
                KeyCode::Char('c') if key.modifiers == KeyModifiers::CONTROL => {
                    return Ok(false);
                }
                _ => {}
            }
        }
    }
}

/// Step 7: Staging sequence — moves files one at a time with TUI redraws.
fn run_staging_sequence(
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    manifest: &staging::StagingManifest,
    session_dir: &std::path::Path,
    config: &Config,
) -> Result<()> {
    let files = &manifest.files;
    let total = files.len();
    let mut total_freed: u64 = 0;

    for (i, staged) in files.iter().enumerate() {
        // Render with this file as active.
        terminal.draw(|f| {
            report::renderer::render_staging_sequence(files, i, Some(i), f);
        })?;

        let freed = staging::stage_one_file(session_dir, staged).unwrap_or(0);
        total_freed += freed;
    }

    // Render all done.
    terminal.draw(|f| {
        report::renderer::render_staging_sequence(files, total, None, f);
    })?;

    // Brief pause to show completed state, then switch to summary.
    std::thread::sleep(Duration::from_millis(600));

    let expiry = staging::expiry_date(&manifest.session_id, config.retention_days)
        .map(|d| d.format("%Y-%m-%d").to_string())
        .unwrap_or_else(|| "unknown".into());

    terminal.draw(|f| {
        report::renderer::render_staging_complete(total, total_freed, &expiry, f);
    })?;

    // Wait for any keypress.
    loop {
        if event::poll(Duration::from_millis(100))? {
            if let Event::Key(_) = event::read()? {
                break;
            }
        }
    }

    Ok(())
}

/// Display a brief message screen and wait for a keypress.
fn run_message_screen(
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    msg: &str,
) -> Result<()> {
    use ratatui::text::{Line, Span};
    use ratatui::widgets::{Block, Paragraph};
    use ratatui::style::Style;

    let msg_owned = msg.to_string();

    terminal.draw(|f| {
        let area = f.area();
        f.render_widget(
            Block::default().style(Style::default().bg(crate::palette::BG)),
            area,
        );
        let lines = vec![
            Line::from(""),
            Line::from(Span::styled(
                format!("  {msg_owned}"),
                Style::default().fg(crate::palette::TEXT),
            )),
            Line::from(""),
            Line::from(Span::styled(
                "  press any key to continue",
                Style::default().fg(crate::palette::CANDLE),
            )),
        ];
        f.render_widget(Paragraph::new(lines).style(Style::default().bg(crate::palette::BG)), area);
    })?;

    loop {
        if event::poll(Duration::from_millis(100))? {
            if let Event::Key(_) = event::read()? {
                break;
            }
        }
    }

    Ok(())
}

// ── cmd_recover ───────────────────────────────────────────────────────────────

/// Interactive recovery: TUI session picker.
fn cmd_recover(recover_all: bool) -> Result<()> {
    let sessions = staging::list_sessions();

    if sessions.is_empty() {
        let mut terminal = enter_tui()?;
        run_message_screen(&mut terminal, "no staged sessions found.")?;
        leave_tui(&mut terminal)?;
        return Ok(());
    }

    if recover_all {
        let mut terminal = enter_tui()?;
        let (session_id, manifest) = sessions.last().context("no sessions")?;
        let total = manifest.files.len();

        terminal.draw(|f| {
            report::renderer::render_recover_result(session_id, 0, total, f);
        })?;

        let n = staging::recover_session(session_id)?;

        terminal.draw(|f| {
            report::renderer::render_recover_result(session_id, n, total, f);
        })?;

        loop {
            if event::poll(Duration::from_millis(100))? {
                if let Event::Key(_) = event::read()? {
                    break;
                }
            }
        }

        leave_tui(&mut terminal)?;
        return Ok(());
    }

    let mut terminal = enter_tui()?;
    let mut cursor = sessions.len().saturating_sub(1); // default to most recent

    loop {
        terminal.draw(|f| {
            report::renderer::render_recover_picker(&sessions, cursor, f);
        })?;

        if !event::poll(Duration::from_millis(50))? {
            continue;
        }

        let Ok(Event::Key(key)) = event::read() else {
            continue;
        };

        match key.code {
            KeyCode::Esc | KeyCode::Char('q') => {
                leave_tui(&mut terminal)?;
                return Ok(());
            }
            KeyCode::Char('c') if key.modifiers == KeyModifiers::CONTROL => {
                leave_tui(&mut terminal)?;
                return Ok(());
            }
            KeyCode::Up if cursor > 0 => {
                cursor = cursor.saturating_sub(1);
            }
            KeyCode::Down if cursor + 1 < sessions.len() => {
                cursor += 1;
            }
            KeyCode::Enter => {
                let (session_id, manifest) = &sessions[cursor];
                let total = manifest.files.len();
                let session_id = session_id.clone();

                terminal.draw(|f| {
                    report::renderer::render_recover_result(&session_id, 0, total, f);
                })?;

                let n = staging::recover_session(&session_id)?;

                terminal.draw(|f| {
                    report::renderer::render_recover_result(&session_id, n, total, f);
                })?;

                loop {
                    if event::poll(Duration::from_millis(100))? {
                        if let Event::Key(_) = event::read()? {
                            break;
                        }
                    }
                }

                leave_tui(&mut terminal)?;
                return Ok(());
            }
            _ => {}
        }
    }
}

// ── cmd_purge ─────────────────────────────────────────────────────────────────

/// Purge staged files, optionally restricted to sessions before a cutoff date.
fn cmd_purge(cutoff: Option<chrono::NaiveDate>) -> Result<()> {
    match cutoff {
        Some(date) => {
            let mut terminal = enter_tui()?;
            let (count, freed) = staging::purge_before(date)?;
            terminal.draw(|f| {
                report::renderer::render_purge_done(count, freed, f);
            })?;
            loop {
                if event::poll(Duration::from_millis(100))? {
                    if let Event::Key(_) = event::read()? {
                        break;
                    }
                }
            }
            leave_tui(&mut terminal)?;
        }
        None => {
            let sessions = staging::list_sessions();

            if sessions.is_empty() {
                let mut terminal = enter_tui()?;
                run_message_screen(&mut terminal, "no staged sessions found.")?;
                leave_tui(&mut terminal)?;
                return Ok(());
            }

            let mut terminal = enter_tui()?;

            loop {
                terminal.draw(|f| {
                    report::renderer::render_purge_confirm(sessions.len(), f);
                })?;

                if !event::poll(Duration::from_millis(50))? {
                    continue;
                }

                let Ok(Event::Key(key)) = event::read() else {
                    continue;
                };

                match key.code {
                    KeyCode::Esc | KeyCode::Char('q') => {
                        leave_tui(&mut terminal)?;
                        return Ok(());
                    }
                    KeyCode::Char('c') if key.modifiers == KeyModifiers::CONTROL => {
                        leave_tui(&mut terminal)?;
                        return Ok(());
                    }
                    KeyCode::Enter => {
                        let mut total_freed = 0u64;
                        for (session_id, _) in &sessions {
                            total_freed += staging::purge_session(session_id).unwrap_or(0);
                        }

                        terminal.draw(|f| {
                            report::renderer::render_purge_done(sessions.len(), total_freed, f);
                        })?;

                        loop {
                            if event::poll(Duration::from_millis(100))? {
                                if let Event::Key(_) = event::read()? {
                                    break;
                                }
                            }
                        }

                        leave_tui(&mut terminal)?;
                        return Ok(());
                    }
                    _ => {}
                }
            }
        }
    }
    Ok(())
}

// ── Config commands ───────────────────────────────────────────────────────────

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

/// Build a short label for the scanned directories.
fn dirs_label(dirs: &[PathBuf]) -> String {
    if dirs.is_empty() {
        return String::new();
    }
    if dirs.len() == 1 {
        // Shorten with ~ if possible.
        let p = dirs[0].to_string_lossy();
        if let Some(home) = home_dir() {
            let home_s = home.to_string_lossy();
            if let Some(rest) = p.strip_prefix(home_s.as_ref()) {
                return format!("~{rest}");
            }
        }
        return p.to_string();
    }
    format!("{} directories", dirs.len())
}
