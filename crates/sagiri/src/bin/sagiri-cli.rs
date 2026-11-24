use std::io::IsTerminal;
use std::path::PathBuf;
use std::process::ExitCode;

use clap::Parser;
use sagiri::cli::{self, Sink, Source};
use sagiri::clipboard::{Selection, SystemClipboard};
use sagiri::{holder, setup};

/// Obfuscates text from stdin or a clipboard selection and writes it to stdout or a
/// clipboard selection.
///
/// Without `--in`, the input is stdin; a terminal on stdin is refused.
#[derive(Parser)]
#[command(name = "sagiri-cli", version)]
struct Cli {
    /// Where to read the input from.
    #[arg(short = 'i', long = "in", value_enum, value_name = "SOURCE")]
    input: Option<Source>,

    /// Where to write the output to.
    #[arg(
        short = 'o',
        long = "out",
        value_enum,
        value_name = "SINK",
        default_value_t = Sink::default()
    )]
    output: Sink,

    /// Remove one trailing line break from the output.
    #[arg(short = 'n', long)]
    trim_newline: bool,

    /// Key file to use instead of `SAGIRI_KEY_FILE` or the default location.
    #[arg(long, value_name = "PATH")]
    key_file: Option<PathBuf>,

    /// Config file to use instead of `SAGIRI_CONFIG` or the default location.
    #[arg(long, value_name = "PATH")]
    config: Option<PathBuf>,

    /// Internal: hold the selection with the text from stdin until it is replaced.
    #[arg(long, value_enum, value_name = "SELECTION", hide = true)]
    hold: Option<Selection>,
}

fn main() -> ExitCode {
    let args = Cli::parse();
    if let Some(selection) = args.hold {
        return hold(selection);
    }
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

/// The holder process. Its stderr is discarded by the parent, so errors only set the
/// exit code.
fn hold(selection: Selection) -> ExitCode {
    let served = holder::serve(
        std::io::stdin().lock(),
        std::io::stdout(),
        &mut holder::SystemHold::default(),
        selection,
    );
    match served {
        Ok(()) => ExitCode::SUCCESS,
        Err(_) => ExitCode::from(1),
    }
}

fn run(args: Cli, source: Source) -> anyhow::Result<()> {
    let engine = setup::build_engine(&setup::Options {
        key_file: args.key_file,
        config: args.config,
    })?;
    let mut write_selection = |selection: Selection, text: &str| {
        if cfg!(target_os = "linux") {
            holder::spawn(selection, text)
        } else {
            cli::write_direct(&mut SystemClipboard::new(), selection, text)
        }
    };
    cli::run(
        &engine,
        cli::Options {
            source,
            sink: args.output,
            trim: args.trim_newline,
        },
        std::io::stdin().lock(),
        &mut SystemClipboard::new(),
        &mut write_selection,
        std::io::stdout().lock(),
    )
}
