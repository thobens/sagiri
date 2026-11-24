use std::io::IsTerminal;
use std::path::PathBuf;
use std::process::ExitCode;

use clap::Parser;
use sagiri::cli::{self, Source};
use sagiri::clipboard::SystemClipboard;
use sagiri::setup;

/// Obfuscates text from stdin or a clipboard selection and writes it to stdout.
///
/// Without `--in`, the input is stdin; a terminal on stdin is refused.
#[derive(Parser)]
#[command(name = "sagiri-cli", version)]
struct Cli {
    /// Where to read the input from.
    #[arg(short = 'i', long = "in", value_enum, value_name = "SOURCE")]
    input: Option<Source>,

    /// Remove one trailing line break from the output.
    #[arg(short = 'n', long)]
    trim_newline: bool,

    /// Key file to use instead of `SAGIRI_KEY_FILE` or the default location.
    #[arg(long, value_name = "PATH")]
    key_file: Option<PathBuf>,

    /// Config file to use instead of `SAGIRI_CONFIG` or the default location.
    #[arg(long, value_name = "PATH")]
    config: Option<PathBuf>,
}

fn main() -> ExitCode {
    let args = Cli::parse();
    let source = match cli::resolve_input(args.input, std::io::stdin().is_terminal()) {
        Ok(source) => source,
        Err(e) => {
            eprintln!("sagiri-cli: {e}");
            return ExitCode::from(2);
        }
    };
    match run(args, source) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("sagiri-cli: {e:#}");
            ExitCode::from(1)
        }
    }
}

fn run(args: Cli, source: Source) -> anyhow::Result<()> {
    let engine = setup::build_engine(&setup::Options {
        key_file: args.key_file,
        config: args.config,
    })?;
    cli::run(
        &engine,
        source,
        args.trim_newline,
        std::io::stdin().lock(),
        &mut SystemClipboard::new(),
        std::io::stdout().lock(),
    )
}
