//! Turns options into an `Engine`. The single place that does so.

use std::path::PathBuf;

use anyhow::Context as _;
use sagiri_core::engine::Engine;
use sagiri_core::key::{Key, resolve_path};
use sagiri_core::pseudo::Pseudonymizer;
use sagiri_core::rules::default_rules;

/// Settings that influence how the engine is built.
#[derive(Debug, Default)]
pub struct Options {
    /// Explicit key file (`--key-file`); wins over `SAGIRI_KEY_FILE` and the XDG path.
    pub key_file: Option<PathBuf>,
}

/// Loads (or creates) the key and builds an engine with the default rules.
pub fn build_engine(opts: &Options) -> anyhow::Result<Engine> {
    let path = resolve_path(opts.key_file.as_deref(), &|k| std::env::var_os(k))
        .context("cannot locate the key file")?;
    let key = Key::load_or_create(&path)?;
    Ok(Engine::new(Pseudonymizer::new(&key), default_rules()))
}
