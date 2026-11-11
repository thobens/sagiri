use clap::{CommandFactory, Parser};

// The `about` text comes from `description` in this crate's Cargo.toml.
#[derive(Parser)]
#[command(version, about)]
struct Cli {}

fn main() {
    let _ = Cli::parse();
    let _ = Cli::command().print_help();
}
