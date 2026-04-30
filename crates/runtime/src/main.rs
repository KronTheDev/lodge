use lodge::engine;
use lodge::shim;
mod tui;

use std::path::Path;

use clap::{Parser, Subcommand};
use crossterm::{
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{backend::CrosstermBackend, Terminal};

const VERSION: &str = env!("CARGO_PKG_VERSION");

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
    /// Install a package from a local path.
    Install {
        /// Path to the package directory containing a lodge.json manifest.
        package: String,
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
        Some(Command::Install { package }) => {
            let pkg_path = Path::new(&package);
            run_install(pkg_path)?;
        }
    }

    Ok(())
}

fn run_install(pkg_path: &Path) -> anyhow::Result<()> {
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

    // Flashcard — confirm or abort
    let confirmed = tui::flashcard::show(&manifest, &plan, &mut terminal)?;

    if !confirmed {
        disable_raw_mode()?;
        execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
        eprintln!("left it.");
        return Ok(());
    }

    // Sequence — execute and display
    let hooks_run = tui::sequence::run(
        &manifest.id,
        &manifest.version,
        &plan,
        pkg_path,
        &mut terminal,
    )?;

    // Restore terminal
    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;

    // Write receipt
    let scope = engine::inference::infer_scope(&manifest, false)?.scope;
    engine::attester::write_receipt(&manifest, &plan, &scope, hooks_run, VERSION)?;

    // Register shim if it's a CLI tool
    use lodge_shared::manifest::PackageType;
    if matches!(manifest.package_type, PackageType::CliTool) {
        if let Some(first_entry) = plan.entries.first() {
            shim::register::register(manifest.command_name(), &first_entry.destination)?;
        }
    }

    println!("{} v{} settled in.", manifest.id, manifest.version);
    Ok(())
}
