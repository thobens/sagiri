mod filter;
mod setup;
mod tui;

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
    /// Interactive mode.
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
            if !std::io::stdout().is_terminal() {
                eprintln!("sagiri: interactive mode needs a terminal");
                return ExitCode::from(2);
            }
            match run_tui(cli) {
                Ok(()) => ExitCode::SUCCESS,
                Err(e) => {
                    eprintln!("sagiri: {e:#}");
                    ExitCode::from(1)
                }
            }
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

fn run_tui(cli: Cli) -> anyhow::Result<()> {
    let engine = setup::build_engine(&setup::Options {
        key_file: cli.key_file,
    })?;
    tui::run(engine)
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
