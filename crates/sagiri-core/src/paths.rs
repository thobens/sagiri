//! XDG base directory resolution. This is the only module that reads path variables.

use std::ffi::OsString;
use std::path::PathBuf;

/// Lookup of an environment variable, injectable for tests.
pub type EnvLookup<'a> = &'a dyn Fn(&str) -> Option<OsString>;

/// Errors resolving a Sagiri path.
#[derive(Debug, thiserror::Error)]
pub enum PathError {
    /// `HOME` was needed but is unset or empty.
    #[error("HOME is not set; set HOME or the relevant XDG_* variable")]
    NoHome,
}

/// `$XDG_CONFIG_HOME/sagiri`, default `$HOME/.config/sagiri`.
pub fn config_dir() -> Result<PathBuf, PathError> {
    config_dir_with(&|k| std::env::var_os(k))
}

/// Like [`config_dir`] with an explicit environment.
pub fn config_dir_with(env: EnvLookup<'_>) -> Result<PathBuf, PathError> {
    xdg_dir(env, "XDG_CONFIG_HOME", ".config")
}

/// `$XDG_DATA_HOME/sagiri`, default `$HOME/.local/share/sagiri`.
pub fn data_dir() -> Result<PathBuf, PathError> {
    data_dir_with(&|k| std::env::var_os(k))
}

/// Like [`data_dir`] with an explicit environment.
pub fn data_dir_with(env: EnvLookup<'_>) -> Result<PathBuf, PathError> {
    xdg_dir(env, "XDG_DATA_HOME", ".local/share")
}

fn xdg_dir(env: EnvLookup<'_>, var: &str, default_rel: &str) -> Result<PathBuf, PathError> {
    if let Some(v) = env(var) {
        let p = PathBuf::from(v);
        if p.is_absolute() {
            return Ok(p.join("sagiri"));
        }
    }
    match env("HOME") {
        Some(h) if !h.is_empty() => Ok(PathBuf::from(h).join(default_rel).join("sagiri")),
        _ => Err(PathError::NoHome),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    fn env_of(pairs: &[(&str, &str)]) -> impl Fn(&str) -> Option<OsString> + use<> {
        let m: HashMap<String, OsString> = pairs
            .iter()
            .map(|(k, v)| (k.to_string(), OsString::from(v)))
            .collect();
        move |k| m.get(k).cloned()
    }

    type Case = (
        &'static str,
        fn(EnvLookup<'_>) -> Result<PathBuf, PathError>,
        &'static str,
    );
    const CASES: [Case; 2] = [
        ("XDG_CONFIG_HOME", config_dir_with, "/home/u/.config/sagiri"),
        (
            "XDG_DATA_HOME",
            data_dir_with,
            "/home/u/.local/share/sagiri",
        ),
    ];

    #[test]
    fn absolute_variable_wins() {
        for (var, f, _) in CASES {
            let env = env_of(&[(var, "/x"), ("HOME", "/home/u")]);
            assert_eq!(f(&env).unwrap(), PathBuf::from("/x/sagiri"));
            // HOME is not needed when the variable is usable.
            let env = env_of(&[(var, "/x")]);
            assert_eq!(f(&env).unwrap(), PathBuf::from("/x/sagiri"));
        }
    }

    #[test]
    fn unset_empty_or_relative_falls_back_to_home() {
        for (var, f, default) in CASES {
            for pairs in [
                vec![("HOME", "/home/u")],
                vec![(var, ""), ("HOME", "/home/u")],
                vec![(var, "rel/dir"), ("HOME", "/home/u")],
            ] {
                let env = env_of(&pairs);
                assert_eq!(f(&env).unwrap(), PathBuf::from(default));
            }
        }
    }

    #[test]
    fn missing_or_empty_home_is_an_error() {
        for (var, f, _) in CASES {
            for pairs in [vec![], vec![("HOME", "")], vec![(var, "rel"), ("HOME", "")]] {
                let env = env_of(&pairs);
                assert!(matches!(f(&env), Err(PathError::NoHome)));
            }
        }
    }
}
