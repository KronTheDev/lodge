mod engine;
mod shim;
mod tui;

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
    /// Install a package from a local path or package ID.
    Install {
        /// Package path or ID (e.g. `./mytool`, `mytool`, `mytool@1.0.0`).
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
            // TODO (M3): resolve, execute, write receipt
            eprintln!("lodge {VERSION} — install {package:?} not yet implemented");
        }
    }

    Ok(())
}
