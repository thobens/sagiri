//! XDG base directory resolution. This is the only module that reads path variables.
//!
//! Each directory has two functions: one that reads the process environment, and a
//! `*_with` variant that takes an [`EnvLookup`] instead, for tests and for callers that
//! keep their own environment. The functions only compute paths; they create nothing.

use std::ffi::OsString;
use std::path::PathBuf;

/// Lookup of an environment variable, injectable for tests.
///
/// Returns the value of the named variable, or `None` if it is unset. For the process
/// environment pass `&|k| std::env::var_os(k)`.
pub type EnvLookup<'a> = &'a dyn Fn(&str) -> Option<OsString>;

/// Errors resolving a Sagiri path.
#[derive(Debug, thiserror::Error)]
pub enum PathError {
    /// `HOME` was needed but is unset or empty.
    #[error("HOME is not set; set HOME or the relevant XDG_* variable")]
    NoHome,
    /// `XDG_RUNTIME_DIR` is unset, empty or relative. There is no fallback.
    #[error("XDG_RUNTIME_DIR is not set to an absolute path")]
    NoRuntimeDir,
}

/// `$XDG_CONFIG_HOME/sagiri`, default `$HOME/.config/sagiri`.
///
/// Reads the process environment; see [`config_dir_with`] for the rules.
///
/// # Errors
///
/// [`PathError::NoHome`] as described for [`config_dir_with`].
pub fn config_dir() -> Result<PathBuf, PathError> {
    config_dir_with(&|k| std::env::var_os(k))
}

/// Like [`config_dir`] with an explicit environment.
///
/// An `XDG_CONFIG_HOME` that is unset, empty or relative is ignored, as the XDG base
/// directory specification requires, and `$HOME/.config/sagiri` is used instead.
///
/// # Errors
///
/// [`PathError::NoHome`] if the fallback is needed and `HOME` is unset or empty.
///
/// ```
/// use sagiri_core::paths::config_dir_with;
/// use std::ffi::OsString;
///
/// let home = tempfile::tempdir()?;
/// let xdg = tempfile::tempdir()?;
///
/// let env = |k: &str| (k == "HOME").then(|| OsString::from(home.path()));
/// assert_eq!(config_dir_with(&env)?, home.path().join(".config/sagiri"));
///
/// let env = |k: &str| match k {
///     "HOME" => Some(OsString::from(home.path())),
///     "XDG_CONFIG_HOME" => Some(OsString::from(xdg.path())),
///     _ => None,
/// };
/// assert_eq!(config_dir_with(&env)?, xdg.path().join("sagiri"));
/// # Ok::<(), Box<dyn std::error::Error>>(())
/// ```
pub fn config_dir_with(env: EnvLookup<'_>) -> Result<PathBuf, PathError> {
    xdg_dir(env, "XDG_CONFIG_HOME", ".config")
}

/// `$XDG_DATA_HOME/sagiri`, default `$HOME/.local/share/sagiri`.
///
/// Reads the process environment; see [`data_dir_with`] for the rules.
///
/// # Errors
///
/// [`PathError::NoHome`] as described for [`data_dir_with`].
pub fn data_dir() -> Result<PathBuf, PathError> {
    data_dir_with(&|k| std::env::var_os(k))
}

/// Like [`data_dir`] with an explicit environment.
///
/// An `XDG_DATA_HOME` that is unset, empty or relative is ignored and
/// `$HOME/.local/share/sagiri` is used instead.
///
/// # Errors
///
/// [`PathError::NoHome`] if the fallback is needed and `HOME` is unset or empty.
///
/// ```
/// use sagiri_core::paths::{PathError, data_dir_with};
/// use std::ffi::OsString;
///
/// let home = tempfile::tempdir()?;
/// let env = |k: &str| match k {
///     "HOME" => Some(OsString::from(home.path())),
///     "XDG_DATA_HOME" => Some(OsString::new()), // empty, so ignored
///     _ => None,
/// };
/// assert_eq!(data_dir_with(&env)?, home.path().join(".local/share/sagiri"));
///
/// let empty = |_: &str| None;
/// assert!(matches!(data_dir_with(&empty), Err(PathError::NoHome)));
/// # Ok::<(), Box<dyn std::error::Error>>(())
/// ```
pub fn data_dir_with(env: EnvLookup<'_>) -> Result<PathBuf, PathError> {
    xdg_dir(env, "XDG_DATA_HOME", ".local/share")
}

/// `$XDG_RUNTIME_DIR/sagiri`. Only computes the path; creates nothing.
///
/// Reads the process environment; see [`runtime_dir_with`] for the rules.
///
/// # Errors
///
/// [`PathError::NoRuntimeDir`] as described for [`runtime_dir_with`].
pub fn runtime_dir() -> Result<PathBuf, PathError> {
    runtime_dir_with(&|k| std::env::var_os(k))
}

/// Like [`runtime_dir`] with an explicit environment.
///
/// An unset, empty or relative `XDG_RUNTIME_DIR` is an error. A shared temporary
/// directory would be unsafe for a control socket, so there is no fallback.
///
/// # Errors
///
/// [`PathError::NoRuntimeDir`] if `XDG_RUNTIME_DIR` is unset, empty or relative.
///
/// ```
/// use sagiri_core::paths::{PathError, runtime_dir_with};
/// use std::ffi::OsString;
///
/// let run = tempfile::tempdir()?;
/// let env = |k: &str| (k == "XDG_RUNTIME_DIR").then(|| OsString::from(run.path()));
/// assert_eq!(runtime_dir_with(&env)?, run.path().join("sagiri"));
///
/// let empty = |_: &str| None;
/// assert!(matches!(runtime_dir_with(&empty), Err(PathError::NoRuntimeDir)));
/// # Ok::<(), Box<dyn std::error::Error>>(())
/// ```
pub fn runtime_dir_with(env: EnvLookup<'_>) -> Result<PathBuf, PathError> {
    match env("XDG_RUNTIME_DIR").map(PathBuf::from) {
        Some(p) if p.is_absolute() => Ok(p.join("sagiri")),
        _ => Err(PathError::NoRuntimeDir),
    }
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

    #[test]
    fn runtime_dir_uses_xdg_runtime_dir() {
        let env = env_of(&[("XDG_RUNTIME_DIR", "/run/user/1000"), ("HOME", "/home/u")]);
        assert_eq!(
            runtime_dir_with(&env).unwrap(),
            PathBuf::from("/run/user/1000/sagiri")
        );
    }

    #[test]
    fn runtime_dir_unset_empty_or_relative_is_error() {
        for pairs in [
            vec![],
            vec![("HOME", "/home/u")],
            vec![("XDG_RUNTIME_DIR", ""), ("HOME", "/home/u")],
            vec![("XDG_RUNTIME_DIR", "run/user/1000"), ("HOME", "/home/u")],
        ] {
            let env = env_of(&pairs);
            assert!(
                matches!(runtime_dir_with(&env), Err(PathError::NoRuntimeDir)),
                "{pairs:?}"
            );
        }
        assert_eq!(
            PathError::NoRuntimeDir.to_string(),
            "XDG_RUNTIME_DIR is not set to an absolute path"
        );
    }
}
