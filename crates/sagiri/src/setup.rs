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

/// The engine and the config it was built from.
pub struct Loaded {
    pub engine: Engine,
    pub config: Config,
}

/// Loads the config, loads (or creates) the key and builds the engine.
pub fn build_engine(opts: &Options) -> anyhow::Result<Engine> {
    load(opts).map(|l| l.engine)
}

/// Like [`build_engine`], and also returns the config.
pub fn load(opts: &Options) -> anyhow::Result<Loaded> {
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
    let engine =
        Engine::new(Pseudonymizer::new(&key), rules).with_allowlist(Allowlist::new(&config.allow)?);
    Ok(Loaded { engine, config })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs::{self, OpenOptions};
    use std::io::Write as _;
    use std::os::unix::fs::OpenOptionsExt as _;

    #[test]
    fn load_returns_config() {
        let dir = tempfile::TempDir::new().unwrap();
        // The test key: bytes 0x00..=0x1f.
        let key_file = dir.path().join("key");
        let hex: String = (0u8..32).map(|b| format!("{b:02x}")).collect();
        OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&key_file)
            .unwrap()
            .write_all(format!("{hex}\n").as_bytes())
            .unwrap();
        let config = dir.path().join("config.toml");
        fs::write(&config, "[session]\nnotify = false\n").unwrap();
        let opts = Options {
            key_file: Some(key_file),
            config: Some(config),
        };
        let loaded = load(&opts).unwrap();
        assert!(!loaded.config.session.notify);
        // Golden vector from `golden_ip.rs`.
        assert_eq!(loaded.engine.obfuscate("10.1.2.3"), "10.154.112.225");
        assert_eq!(
            build_engine(&opts).unwrap().obfuscate("10.1.2.3"),
            "10.154.112.225"
        );
    }
}
