use std::path::PathBuf;
use std::process::ExitCode;
use std::sync::mpsc::Sender;
use std::thread;

use anyhow::Context as _;
use clap::{Parser, Subcommand};
use sagiri::clipboard::SystemClipboard;
use sagiri::session::backend;
use sagiri::session::control;
use sagiri::session::core::{Command, Event, Session};
use sagiri::session::daemon::{self, Parts};
use sagiri::session::notify::Notifiers;
use sagiri::setup;
use sagiri_core::config::SessionConfig;
use sagiri_core::paths;
use signal_hook::consts::{SIGINT, SIGTERM};
use signal_hook::iterator::Signals;

/// Runs an obfuscating clipboard session, or controls the one that runs.
///
/// While a session is active, text copied to the clipboard is replaced by its obfuscated
/// form.
#[derive(Parser)]
#[command(name = "sagiri-session", version)]
struct Cli {
    /// Control socket to use instead of `$XDG_RUNTIME_DIR/sagiri/session.sock`.
    #[arg(long, global = true, value_name = "PATH")]
    socket: Option<PathBuf>,

    #[command(subcommand)]
    command: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// Run the session process until `quit`, SIGINT or SIGTERM.
    Run {
        /// Start the session right away.
        #[arg(long)]
        active: bool,

        /// Key file to use instead of `SAGIRI_KEY_FILE` or the default location.
        #[arg(long, value_name = "PATH")]
        key_file: Option<PathBuf>,

        /// Config file to use instead of `SAGIRI_CONFIG` or the default location.
        #[arg(long, value_name = "PATH")]
        config: Option<PathBuf>,
    },
    /// Start the session.
    Start,
    /// Stop the session.
    Stop,
    /// Start the session if it is stopped, stop it otherwise.
    Toggle,
    /// Print the state of the session.
    Status,
    /// End the session process.
    Quit,
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    match run(cli) {
        Ok(code) => ExitCode::from(code),
        Err(e) => {
            eprintln!("sagiri-session: {e:#}");
            ExitCode::from(1)
        }
    }
}

fn run(cli: Cli) -> anyhow::Result<u8> {
    let socket = socket_path(cli.socket)?;
    let command = match cli.command {
        Cmd::Run {
            active,
            key_file,
            config,
        } => return run_daemon(socket, active, setup::Options { key_file, config }),
        Cmd::Start => Command::Start,
        Cmd::Stop => Command::Stop,
        Cmd::Toggle => Command::Toggle,
        Cmd::Status => Command::Status,
        Cmd::Quit => Command::Quit,
    };
    let active = control::request(&socket, command)?;
    println!("{}", control::reply_line(active));
    Ok(0)
}

fn socket_path(explicit: Option<PathBuf>) -> anyhow::Result<PathBuf> {
    match explicit {
        Some(path) => Ok(path),
        None => match paths::runtime_dir() {
            Ok(dir) => Ok(dir.join("session.sock")),
            Err(e) => anyhow::bail!("{e}; use --socket <PATH>"),
        },
    }
}

fn run_daemon(socket: PathBuf, active: bool, opts: setup::Options) -> anyhow::Result<u8> {
    let loaded = setup::load(&opts)?;
    let settings: SessionConfig = loaded.config.session;
    let session = Session::new(
        loaded.engine,
        Box::new(SystemClipboard::new()),
        Box::new(Notifiers::for_session(&settings)),
    );
    let exit = daemon::run(Parts {
        session,
        socket,
        start_active: active,
        sources: Box::new(move |events| {
            watch_signals(events.clone())?;
            backend::start(events, &settings)
        }),
    })?;
    Ok(daemon::exit_code(exit))
}

/// Turns SIGINT and SIGTERM into `Event::Shutdown`.
fn watch_signals(events: Sender<Event>) -> anyhow::Result<()> {
    let mut signals =
        Signals::new([SIGINT, SIGTERM]).context("cannot install the signal handlers")?;
    thread::Builder::new()
        .name("signals".into())
        .spawn(move || {
            for _ in signals.forever() {
                if events.send(Event::Shutdown).is_err() {
                    break;
                }
            }
        })
        .context("cannot start the signal thread")?;
    Ok(())
}
