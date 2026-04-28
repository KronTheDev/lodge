mod engine;
mod shim;
mod tui;

use std::path::Path;

use clap::{Parser, Subcommand};

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
            // TODO (M4): launch TUI command bar with splash screen
            eprintln!("lodge {VERSION} — command bar not yet implemented");
        }
        Some(Command::Install { package }) => {
            let pkg_path = Path::new(&package);
            let manifest_path = pkg_path.join("lodge.json");
            let json = std::fs::read_to_string(&manifest_path).map_err(|e| {
                anyhow::anyhow!("couldn't read lodge.json in {:?}: {e}", pkg_path)
            })?;
            let manifest = engine::manifest::parse(&json)?;
            let os = engine::resolver::current_os();
            let plan = engine::resolver::resolve(pkg_path, &manifest, os, false)?;

            println!(
                "{} v{}  —  {} files to place",
                manifest.id,
                manifest.version,
                plan.entries.len()
            );
            for entry in &plan.entries {
                println!(
                    "  {} → {}",
                    entry.source.file_name().unwrap_or_default().to_string_lossy(),
                    entry.destination.display()
                );
            }
            // TODO (M3): execute plan, write receipt
            eprintln!("execution not yet implemented — plan only");
        }
    }

    Ok(())
}
