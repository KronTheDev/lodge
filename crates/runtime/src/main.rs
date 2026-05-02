use lodge::engine;
use lodge::shim;
use lodge::VERSION;
mod tui;

use std::path::Path;

use clap::{Parser, Subcommand};
use crossterm::{
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{backend::CrosstermBackend, Terminal};

#[derive(Parser, Debug)]
#[command(
    name = "lodge",
    version = VERSION,
    about = "a place for everything",
    long_about = None,
)]
struct Cli {
    #[command(subcommand)]
    command: Option<Command>,
}

#[derive(Subcommand, Debug)]
enum Command {
    /// Install a package. Accepts a local path or a feed package id.
    Install {
        /// Package id (looked up in the local feed) or path to a package directory.
        target: String,
    },
    /// Remove an installed package.
    Uninstall {
        /// Package id to remove.
        id: String,
    },
    /// Update a package to the latest version in the local feed.
    Update {
        /// Package id to update, or "all" to update every installed package.
        id: String,
    },
    /// Roll back a package to its previous installed version.
    Rollback {
        /// Package id to roll back.
        id: String,
    },
    /// Search the local feed for packages matching a query.
    Search {
        /// Search query (matches id and description).
        query: String,
    },
    /// Switch the active version shim for an installed package.
    Use {
        /// Version spec in the form id@version (e.g. mytool@1.0.0).
        spec: String,
    },
    /// Open the interactive command bar (default when no subcommand is given).
    Bar,
}

fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();

    match cli.command {
        None | Some(Command::Bar) => {
            tui::bar::run()?;
        }
        Some(Command::Install { target }) => {
            run_install_target(&target)?;
        }
        Some(Command::Uninstall { id }) => {
            run_uninstall_cli(&id)?;
        }
        Some(Command::Update { id }) => {
            run_update_cli(&id)?;
        }
        Some(Command::Rollback { id }) => {
            run_rollback_cli(&id)?;
        }
        Some(Command::Search { query }) => {
            run_search_cli(&query);
        }
        Some(Command::Use { spec }) => {
            run_use_cli(&spec)?;
        }
    }

    Ok(())
}

/// Installs from a local path or resolves `target` through the feed.
fn run_install_target(target: &str) -> anyhow::Result<()> {
    let pkg_path = if looks_like_path(target) {
        std::path::PathBuf::from(target)
    } else {
        // Look up in local feed
        let entry = engine::feed::find_latest(target).ok_or_else(|| {
            anyhow::anyhow!(
                "'{target}' is not a path and was not found in the local feed.\n\
                 hint: add the package to {} or pass a directory path.",
                engine::feed::feed_dir().display()
            )
        })?;
        entry.path
    };

    run_install(&pkg_path)
}

fn run_install(pkg_path: &std::path::Path) -> anyhow::Result<()> {
    let manifest_path = pkg_path.join("lodge.json");
    let json = std::fs::read_to_string(&manifest_path)
        .map_err(|e| anyhow::anyhow!("couldn't read lodge.json in {:?}: {e}", pkg_path))?;
    let manifest = engine::manifest::parse(&json)?;

    let os = engine::resolver::current_os();
    let plan = engine::resolver::resolve(pkg_path, &manifest, os, false)?;

    // Set up terminal
    enable_raw_mode()?;
    let mut stdout = std::io::stdout();
    execute!(stdout, EnterAlternateScreen)?;

    let original_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let _ = disable_raw_mode();
        let _ = execute!(std::io::stderr(), LeaveAlternateScreen);
        original_hook(info);
    }));

    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let confirmed = tui::flashcard::show(&manifest, &plan, &mut terminal)?;

    if !confirmed {
        disable_raw_mode()?;
        execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
        eprintln!("left it.");
        return Ok(());
    }

    let hooks_run = tui::sequence::run(
        &manifest.id,
        &manifest.version,
        &plan,
        pkg_path,
        &mut terminal,
    )?;

    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;

    let scope = engine::inference::infer_scope(&manifest, false)?.scope;
    engine::attester::write_receipt(&manifest, &plan, &scope, hooks_run, VERSION)?;

    use lodge_shared::manifest::PackageType;
    if matches!(manifest.package_type, PackageType::CliTool) {
        if let Some(first_entry) = plan.entries.first() {
            shim::register::register(manifest.command_name(), &first_entry.destination)?;
        }
        if let Err(e) = shim::register::ensure_shim_dir_on_path() {
            eprintln!("note: couldn't add shim dir to PATH: {e}");
        }
    }

    println!("{} v{} settled in.", manifest.id, manifest.version);
    Ok(())
}

/// Removes an installed package.
fn run_uninstall_cli(id: &str) -> anyhow::Result<()> {
    let result = engine::uninstall::uninstall(id)?;
    println!("{id} removed.");
    if !result.missing_files.is_empty() {
        println!("  {} file(s) were already gone.", result.missing_files.len());
    }
    if result.shim_removed {
        println!("  shim unregistered.");
    }
    Ok(())
}

/// Updates a package (or all packages) from the local feed.
fn run_update_cli(id: &str) -> anyhow::Result<()> {
    if id.eq_ignore_ascii_case("all") {
        let results = engine::update::update_all(VERSION);
        if results.is_empty() {
            println!("no packages installed.");
        }
        for (pkg_id, result) in &results {
            match result {
                Ok(r) => println!("{}", engine::update::format_update_result(pkg_id, r)),
                Err(e) => eprintln!("{pkg_id}: {e}"),
            }
        }
    } else {
        let result = engine::update::update(id, VERSION)?;
        println!("{}", engine::update::format_update_result(id, &result));
    }
    Ok(())
}

/// Rolls back a package to its previous version.
fn run_rollback_cli(id: &str) -> anyhow::Result<()> {
    let result = engine::rollback::rollback(id, VERSION)?;
    println!("{}", engine::rollback::format_rollback_result(id, &result));
    Ok(())
}

/// Searches the local feed and prints matching packages.
fn run_search_cli(query: &str) {
    let results = engine::feed::search(query);
    println!("{}", engine::feed::format_search_results(&results));
}

/// Switches the active version shim for an installed package.
fn run_use_cli(spec: &str) -> anyhow::Result<()> {
    let (id, version) = parse_version_spec(spec)
        .ok_or_else(|| anyhow::anyhow!("invalid spec '{spec}' — expected id@version"))?;

    let receipts = engine::attester::list_receipts();
    let receipt = receipts
        .into_iter()
        .find(|r| r.id == id && r.version.starts_with(version))
        .ok_or_else(|| anyhow::anyhow!("no installed version of {id} matching {version}"))?;

    let placed = receipt
        .placements
        .first()
        .ok_or_else(|| anyhow::anyhow!("no placed files in receipt for {id}"))?;

    let target = Path::new(&placed.destination);
    shim::register::update(id, target)?;

    println!("shim updated — {id} now resolves to v{}.", receipt.version);
    Ok(())
}

/// Returns `true` when `s` looks like a filesystem path rather than a package id.
fn looks_like_path(s: &str) -> bool {
    s.starts_with('.')
        || s.starts_with('/')
        || s.starts_with('~')
        || s.contains('\\')
        || (s.len() >= 3 && s.chars().nth(1) == Some(':'))
}

/// Parses `id@version` into `(id, version)`. Returns `None` if `@` is absent.
fn parse_version_spec(spec: &str) -> Option<(&str, &str)> {
    let at = spec.rfind('@')?;
    Some((&spec[..at], &spec[at + 1..]))
}
