mod filter;
mod setup;

use std::io::IsTerminal;
use std::path::PathBuf;
use std::process::ExitCode;

use clap::Parser;

// The `about` text comes from `description` in this crate's Cargo.toml.
#[derive(Parser)]
#[command(version, about)]
struct Cli {
    /// Key file to use instead of `SAGIRI_KEY_FILE` or the default location.
    #[arg(long, global = true, value_name = "PATH")]
    key_file: Option<PathBuf>,
}

#[derive(Debug, PartialEq, Eq)]
enum Mode {
    Filter,
    /// Interactive mode; not implemented yet (ticket 0010).
    Terminal,
}

fn mode(stdin_is_terminal: bool) -> Mode {
    if stdin_is_terminal {
        Mode::Terminal
    } else {
        Mode::Filter
    }
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    match mode(std::io::stdin().is_terminal()) {
        Mode::Terminal => {
            eprintln!("sagiri: interactive mode is not available yet; pipe text into sagiri");
            ExitCode::from(2)
        }
        Mode::Filter => match run_filter(cli) {
            Ok(()) => ExitCode::SUCCESS,
            Err(e) => {
                eprintln!("sagiri: {e:#}");
                ExitCode::from(1)
            }
        },
    }
}

fn run_filter(cli: Cli) -> anyhow::Result<()> {
    let engine = setup::build_engine(&setup::Options {
        key_file: cli.key_file,
    })?;
    filter::run(&engine, std::io::stdin().lock(), std::io::stdout().lock())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mode_decision() {
        assert_eq!(mode(false), Mode::Filter);
        assert_eq!(mode(true), Mode::Terminal);
    }
}
