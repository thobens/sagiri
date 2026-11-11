use clap::{CommandFactory, Parser};

/// Sagiri replaces sensitive values in pasted text with consistent, realistic-looking fakes.
#[derive(Parser)]
#[command(version, about)]
struct Cli {}

fn main() {
    let _ = Cli::parse();
    let _ = Cli::command().print_help();
}
