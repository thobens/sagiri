//! Turns options into an `Engine`. The single place that does so.

use std::path::PathBuf;

use anyhow::Context as _;
use sagiri_core::config::{self, Allowlist, Config};
use sagiri_core::engine::Engine;
use sagiri_core::key::{Key, resolve_path};
use sagiri_core::pseudo::Pseudonymizer;
use sagiri_core::rule::Rule;
use sagiri_core::rules::default_rules;
use sagiri_core::rules::term::TermRule;

/// Settings that influence how the engine is built.
#[derive(Debug, Default)]
pub struct Options {
    /// Explicit key file (`--key-file`); wins over `SAGIRI_KEY_FILE` and the XDG path.
    pub key_file: Option<PathBuf>,
    /// Explicit config file (`--config`); wins over `SAGIRI_CONFIG` and the XDG path.
    pub config: Option<PathBuf>,
}

/// Loads the config, loads (or creates) the key and builds the engine.
pub fn build_engine(opts: &Options) -> anyhow::Result<Engine> {
    let path = resolve_path(opts.key_file.as_deref(), &|k| std::env::var_os(k))
        .context("cannot locate the key file")?;
    let config = match config::resolve_path(opts.config.as_deref(), &|k| std::env::var_os(k))? {
        Some(p) => Config::load(&p)?,
        None => Config::default(),
    };
    let key = Key::load_or_create(&path)?;
    let mut rules: Vec<Box<dyn Rule>> = default_rules()
        .into_iter()
        .filter(|r| config.rules.enabled(r.kind()))
        .collect();
    if config.rules.term && !config.terms.is_empty() {
        rules.insert(0, Box::new(TermRule::new(&config.terms)?));
    }
    Ok(Engine::new(Pseudonymizer::new(&key), rules).with_allowlist(Allowlist::new(&config.allow)?))
}
