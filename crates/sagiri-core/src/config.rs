//! The config file: allowlist, terms and rule toggles.
//!
//! Error messages name the file, the key or array index and the position. They never
//! contain a value from the file.
//!
//! A [`Config`] is plain data. [`Allowlist::new`] and
//! [`TermRule::new`](crate::rules::term::TermRule::new) turn its entries into what an
//! [`Engine`](crate::engine::Engine) uses, and [`RuleToggles::enabled`] selects the rules.

use std::ffi::OsString;
use std::fmt;
use std::net::{IpAddr, Ipv6Addr};
use std::path::{Path, PathBuf};
use std::str::FromStr;

use serde::Deserialize;

use crate::paths::{self, PathError};
use crate::rule::Kind;
use crate::rules::ip::parse_v4_lenient;

/// Errors loading or validating the config.
#[derive(Debug, thiserror::Error)]
pub enum ConfigError {
    /// A config file named by flag or environment variable does not exist.
    #[error("config file {} does not exist", .0.display())]
    NotFound(PathBuf),
    /// The file exists but cannot be read.
    #[error("cannot read config file {}: {kind}", .path.display())]
    Read {
        /// The config file.
        path: PathBuf,
        /// What went wrong.
        kind: std::io::ErrorKind,
    },
    /// Syntax error or wrong structure in the TOML.
    #[error("{}: line {line}, column {column}: {detail}", .path.display())]
    Toml {
        /// The config file.
        path: PathBuf,
        /// Line of the error, counting from 1.
        line: usize,
        /// Column of the error in characters, counting from 1.
        column: usize,
        /// What is wrong, naming the key. Never contains a value from the file.
        detail: String,
    },
    /// An entry in `allow` or `terms` is not usable.
    #[error("{}{key}: {reason}", PathPrefix(.path))]
    Invalid {
        /// The config file; `None` if the entries did not come from a file.
        path: Option<PathBuf>,
        /// For example `allow[2]`.
        key: String,
        /// What is wrong, as a fixed text.
        reason: &'static str,
    },
}

struct PathPrefix<'a>(&'a Option<PathBuf>);

impl fmt::Display for PathPrefix<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.0 {
            Some(p) => write!(f, "{}: ", p.display()),
            None => Ok(()),
        }
    }
}

impl ConfigError {
    pub(crate) fn invalid(key: String, reason: &'static str) -> Self {
        ConfigError::Invalid {
            path: None,
            key,
            reason,
        }
    }

    fn with_path(self, file: &Path) -> Self {
        match self {
            ConfigError::Invalid {
                path: None,
                key,
                reason,
            } => ConfigError::Invalid {
                path: Some(file.to_path_buf()),
                key,
                reason,
            },
            other => other,
        }
    }
}

/// The parsed config file. Every field has a default.
///
/// [`Config::default`] is the configuration without a file: no allowlist, no terms,
/// every rule on.
#[derive(Debug, Clone, PartialEq, Eq, Default, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct Config {
    /// Values that are never replaced.
    pub allow: Vec<String>,
    /// Literal terms that are always replaced.
    pub terms: Vec<String>,
    /// Which rules are switched on.
    pub rules: RuleToggles,
    /// Settings read by `sagiri-session` only.
    pub session: SessionConfig,
}

/// Settings of session mode. Other binaries ignore them.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct SessionConfig {
    /// The hotkey as written in the file; its syntax is checked elsewhere.
    pub hotkey: Option<String>,
    /// How the PRIMARY selection is treated.
    pub primary: PrimaryMode,
    /// Whether desktop notifications are shown.
    pub notify: bool,
}

impl Default for SessionConfig {
    fn default() -> Self {
        SessionConfig {
            hotkey: None,
            primary: PrimaryMode::Off,
            notify: true,
        }
    }
}

/// How session mode treats the PRIMARY selection.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PrimaryMode {
    /// PRIMARY is left alone.
    #[default]
    Off,
    /// Text that the session writes to the clipboard is written to PRIMARY too.
    Mirror,
    /// Experimental: text selected on PRIMARY is replaced there, without a copy.
    Watch,
}

/// One switch per rule. Rules are on unless switched off.
///
/// The field names are the [`Kind::id`] values and the keys of the `[rules]` table.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct RuleToggles {
    /// The rule for [`Kind::Term`].
    pub term: bool,
    /// The rule for [`Kind::Url`].
    pub url: bool,
    /// The rule for [`Kind::Email`].
    pub email: bool,
    /// The rule for [`Kind::Ipv4`].
    pub ipv4: bool,
    /// The rule for [`Kind::Ipv6`].
    pub ipv6: bool,
    /// The rule for [`Kind::Hostname`].
    pub hostname: bool,
    /// The rule for [`Kind::PaymentCard`].
    pub payment_card: bool,
    /// The rule for [`Kind::Iban`].
    pub iban: bool,
    /// The rule for [`Kind::Phone`].
    pub phone: bool,
}

impl Default for RuleToggles {
    fn default() -> Self {
        RuleToggles {
            term: true,
            url: true,
            email: true,
            ipv4: true,
            ipv6: true,
            hostname: true,
            payment_card: true,
            iban: true,
            phone: true,
        }
    }
}

impl RuleToggles {
    /// True if the rule for `kind` is switched on.
    ///
    /// ```
    /// use sagiri_core::config::RuleToggles;
    /// use sagiri_core::rule::Kind;
    /// use sagiri_core::rules::default_rules;
    ///
    /// let toggles = RuleToggles { phone: false, ..RuleToggles::default() };
    /// assert!(toggles.enabled(Kind::Ipv4));
    /// assert!(!toggles.enabled(Kind::Phone));
    ///
    /// let rules: Vec<_> = default_rules()
    ///     .into_iter()
    ///     .filter(|r| toggles.enabled(r.kind()))
    ///     .collect();
    /// assert_eq!(rules.len(), default_rules().len() - 1);
    /// ```
    pub fn enabled(&self, kind: Kind) -> bool {
        match kind {
            Kind::Term => self.term,
            Kind::Url => self.url,
            Kind::Email => self.email,
            Kind::Ipv4 => self.ipv4,
            Kind::Ipv6 => self.ipv6,
            Kind::Hostname => self.hostname,
            Kind::PaymentCard => self.payment_card,
            Kind::Iban => self.iban,
            Kind::Phone => self.phone,
        }
    }
}

impl Config {
    /// Reads and validates the file at `path`.
    ///
    /// The file is TOML. Unknown keys are errors, missing keys take their defaults. The
    /// entries of `allow` and `terms` are checked here, so that a config that loads also
    /// builds an [`Allowlist`] and a [`TermRule`](crate::rules::term::TermRule).
    ///
    /// # Errors
    ///
    /// - [`ConfigError::NotFound`] if there is no file at `path`.
    /// - [`ConfigError::Read`] if the file cannot be read.
    /// - [`ConfigError::Toml`] for a syntax error, an unknown key or a value of the wrong
    ///   type.
    /// - [`ConfigError::Invalid`] for an entry of `allow` or `terms` that cannot be used.
    ///
    /// ```
    /// use sagiri_core::config::{Config, ConfigError};
    ///
    /// let dir = tempfile::tempdir()?;
    /// let path = dir.path().join("config.toml");
    /// std::fs::write(&path, "allow = [\"*.example.net\"]\n[rules]\nphone = false\n")?;
    ///
    /// let config = Config::load(&path)?;
    /// assert_eq!(config.allow, ["*.example.net"]);
    /// assert!(!config.rules.phone);
    /// assert!(config.rules.ipv4);
    ///
    /// std::fs::write(&path, "terms = [\"x\"]\n")?;
    /// let err = Config::load(&path).unwrap_err();
    /// assert!(matches!(err, ConfigError::Invalid { ref key, .. } if key == "terms[0]"));
    /// # Ok::<(), Box<dyn std::error::Error>>(())
    /// ```
    pub fn load(path: &Path) -> Result<Config, ConfigError> {
        let src = std::fs::read_to_string(path).map_err(|e| match e.kind() {
            std::io::ErrorKind::NotFound => ConfigError::NotFound(path.to_path_buf()),
            kind => ConfigError::Read {
                path: path.to_path_buf(),
                kind,
            },
        })?;
        let config = parse(&src, path)?;
        // Validate entries now so that errors name the file.
        Allowlist::new(&config.allow).map_err(|e| e.with_path(path))?;
        crate::rules::term::validate_terms(&config.terms).map_err(|e| e.with_path(path))?;
        Ok(config)
    }
}

/// Where the config file is: the flag, else `SAGIRI_CONFIG`, else the XDG path.
///
/// `None` means that there is no file and the defaults apply. A file named by flag or
/// environment variable must exist; a missing file at the XDG path is not an error.
///
/// `env` looks up environment variables; pass `&|k| std::env::var_os(k)` for the process
/// environment. An empty `SAGIRI_CONFIG` counts as unset. The XDG path is
/// `config.toml` in [`config_dir_with`](paths::config_dir_with); without a usable `HOME`
/// there is no XDG path, and the result is `None`.
///
/// # Errors
///
/// [`ConfigError::NotFound`] if the file named by the flag or by `SAGIRI_CONFIG` does not
/// exist.
///
/// ```
/// use sagiri_core::config::{ConfigError, resolve_path};
/// use std::ffi::OsString;
///
/// let home = tempfile::tempdir()?;
/// let env = |k: &str| (k == "HOME").then(|| OsString::from(home.path()));
///
/// // No file at the XDG path: the defaults apply.
/// assert_eq!(resolve_path(None, &env)?, None);
///
/// let file = home.path().join(".config/sagiri/config.toml");
/// std::fs::create_dir_all(file.parent().unwrap())?;
/// std::fs::write(&file, "")?;
/// assert_eq!(resolve_path(None, &env)?, Some(file));
///
/// // A file named by flag must exist.
/// let missing = home.path().join("missing.toml");
/// assert!(matches!(resolve_path(Some(&missing), &env), Err(ConfigError::NotFound(_))));
/// # Ok::<(), Box<dyn std::error::Error>>(())
/// ```
pub fn resolve_path(
    flag: Option<&Path>,
    env: &dyn Fn(&str) -> Option<OsString>,
) -> Result<Option<PathBuf>, ConfigError> {
    let explicit = match flag {
        Some(p) => Some(p.to_path_buf()),
        None => env("SAGIRI_CONFIG")
            .filter(|v| !v.is_empty())
            .map(PathBuf::from),
    };
    if let Some(p) = explicit {
        return if p.exists() {
            Ok(Some(p))
        } else {
            Err(ConfigError::NotFound(p))
        };
    }
    // Without HOME and a usable XDG_CONFIG_HOME there is no default location: no file.
    let dir = match paths::config_dir_with(env) {
        Ok(dir) => dir,
        Err(PathError::NoHome) => return Ok(None),
        // Never returned by `config_dir_with`; listed only to keep the match exhaustive.
        Err(PathError::NoRuntimeDir) => return Ok(None),
    };
    let p = dir.join("config.toml");
    Ok(p.exists().then_some(p))
}

fn position(src: &str, offset: usize) -> (usize, usize) {
    let offset = offset.min(src.len());
    let before = src.get(..offset).unwrap_or("");
    let line = before.matches('\n').count() + 1;
    let column = before.rsplit('\n').next().map_or(0, |l| l.chars().count()) + 1;
    (line, column)
}

fn parse(src: &str, path: &Path) -> Result<Config, ConfigError> {
    let err = |offset: Option<usize>, detail: String| {
        let (line, column) = position(src, offset.unwrap_or(0));
        ConfigError::Toml {
            path: path.to_path_buf(),
            line,
            column,
            detail,
        }
    };
    let table = toml::de::DeTable::parse(src)
        .map_err(|e| err(e.span().map(|s| s.start), e.message().to_string()))?;
    // serde also accepts an array for a struct and fills the fields in order, so
    // `rules` and `session` must be checked to be tables before deserializing. A later
    // key whose value is a table belongs in this list as well.
    for key in ["rules", "session"] {
        if let Some(v) = table.get_ref().get(key)
            && !matches!(v.get_ref(), toml::de::DeValue::Table(_))
        {
            return Err(err(
                Some(v.span().start),
                format!("`{key}`: expected a table"),
            ));
        }
    }
    toml::from_str::<Config>(src).map_err(|e| {
        let start = e.span().map(|s| s.start);
        // The message of an unknown field names only the key. Other messages quote the
        // offending value, so they are replaced by one that names the key.
        let detail = if e.message().starts_with("unknown field") {
            e.message().to_string()
        } else if let Some((key, expected)) = start.and_then(|o| locate(table.get_ref(), o)) {
            format!("`{key}`: {expected}")
        } else {
            "invalid value".to_string()
        };
        err(start, detail)
    })
}

/// The key or array element whose value contains `offset`, and what is expected there.
fn locate(table: &toml::de::DeTable<'_>, offset: usize) -> Option<(String, &'static str)> {
    use toml::de::DeValue;
    for (k, v) in table {
        let key: &str = k.get_ref().as_ref();
        // The span of a `[table]` covers its header only, so look inside it.
        if !v.span().contains(&offset) && key != "rules" && key != "session" {
            continue;
        }
        let found = match (key, v.get_ref()) {
            ("allow" | "terms", DeValue::Array(items)) => {
                let i = items.iter().position(|x| x.span().contains(&offset))?;
                Some((format!("{key}[{i}]"), "expected a string"))
            }
            ("allow" | "terms", _) => Some((key.to_string(), "expected an array of strings")),
            ("rules", DeValue::Table(rules)) => rules
                .iter()
                .find(|(_, x)| x.span().contains(&offset))
                .map(|(rk, _)| {
                    let rk: &str = rk.get_ref().as_ref();
                    (format!("rules.{rk}"), "expected a boolean")
                }),
            ("session", DeValue::Table(session)) => session
                .iter()
                .find(|(_, x)| x.span().contains(&offset))
                .map(|(sk, _)| {
                    let sk: &str = sk.get_ref().as_ref();
                    let expected = match sk {
                        "hotkey" => "expected a string",
                        "primary" => r#"expected "off", "mirror" or "watch""#,
                        "notify" => "expected a boolean",
                        _ => "invalid value",
                    };
                    (format!("session.{sk}"), expected)
                }),
            // A `rules` or `session` that is not a table is rejected in `parse` before
            // this runs.
            _ => None,
        };
        if found.is_some() {
            return found;
        }
    }
    None
}

enum Entry {
    /// Lower-case text and its address, if it is one.
    Exact(String, Option<IpAddr>),
    /// `.d` for `*.d`, lower-case.
    Subdomains(String),
    Network(IpAddr, u8),
}

/// Values that are never replaced.
///
/// `Debug` shows the number of entries, never the entries themselves.
pub struct Allowlist {
    entries: Vec<Entry>,
}

// Never prints entries.
impl fmt::Debug for Allowlist {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Allowlist")
            .field("entries", &self.entries.len())
            .finish()
    }
}

fn parse_ip(s: &str) -> Option<IpAddr> {
    parse_v4_lenient(s)
        .map(IpAddr::V4)
        .or_else(|| Ipv6Addr::from_str(s).ok().map(IpAddr::V6))
}

fn parse_network(s: &str) -> Option<(IpAddr, u8)> {
    let (addr, bits) = s.split_once('/')?;
    let addr = parse_ip(addr)?;
    if bits.is_empty() || bits.len() > 3 || !bits.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let bits: u8 = bits.parse().ok()?;
    let max = if addr.is_ipv4() { 32 } else { 128 };
    (bits <= max).then_some((addr, bits))
}

fn in_network(addr: IpAddr, net: IpAddr, bits: u8) -> bool {
    match (addr, net) {
        (IpAddr::V4(a), IpAddr::V4(n)) => {
            let mask = u32::MAX.checked_shl(32 - u32::from(bits)).unwrap_or(0);
            u32::from(a) & mask == u32::from(n) & mask
        }
        (IpAddr::V6(a), IpAddr::V6(n)) => {
            let mask = u128::MAX.checked_shl(128 - u32::from(bits)).unwrap_or(0);
            u128::from(a) & mask == u128::from(n) & mask
        }
        _ => false,
    }
}

impl Allowlist {
    /// An allowlist that matches nothing.
    pub fn empty() -> Self {
        Allowlist {
            entries: Vec::new(),
        }
    }

    /// Builds the list from `Config::allow`. An invalid entry is an error naming
    /// `allow[index]`.
    ///
    /// Entries are trimmed. Each entry is one of:
    ///
    /// - a network such as `192.0.2.0/24` or `2001:db8::/32`, covering every address in it;
    /// - a wildcard such as `*.example.com`, covering every name below `example.com` but
    ///   not `example.com` itself;
    /// - any other text, covering exactly that value, ignoring ASCII case. An entry that is
    ///   an IP address also covers other spellings of the same address.
    ///
    /// # Errors
    ///
    /// [`ConfigError::Invalid`] with key `allow[index]` and no path, for an entry that is
    /// empty, contains a line break, or is a malformed network or wildcard.
    ///
    /// ```
    /// use sagiri_core::config::Allowlist;
    ///
    /// let entries = ["192.0.2.0/24", "*.example.com", "Jane.Doe@example.org"].map(String::from);
    /// let allow = Allowlist::new(&entries)?;
    ///
    /// assert!(allow.matches("192.0.2.10"));
    /// assert!(allow.matches("www.example.com"));
    /// assert!(!allow.matches("example.com"));
    /// assert!(allow.matches("jane.doe@EXAMPLE.org"));
    /// assert!(!allow.matches("198.51.100.7"));
    ///
    /// assert!(Allowlist::new(&["192.0.2.0/33".to_string()]).is_err());
    /// # Ok::<(), Box<dyn std::error::Error>>(())
    /// ```
    pub fn new(entries: &[String]) -> Result<Self, ConfigError> {
        let mut out = Vec::with_capacity(entries.len());
        for (i, raw) in entries.iter().enumerate() {
            let bad = |reason| ConfigError::invalid(format!("allow[{i}]"), reason);
            let e = raw.trim();
            if e.is_empty() {
                return Err(bad("empty entry"));
            }
            if e.contains(['\n', '\r']) {
                return Err(bad("entry contains a line break"));
            }
            if e.contains('/') {
                let (addr, bits) = parse_network(e).ok_or_else(|| bad("malformed network"))?;
                out.push(Entry::Network(addr, bits));
            } else if let Some(domain) = e.strip_prefix("*.") {
                let valid = !domain.is_empty()
                    && !domain.starts_with('.')
                    && !domain.ends_with('.')
                    && domain
                        .chars()
                        .all(|c| c.is_alphanumeric() || matches!(c, '-' | '_' | '.'));
                if !valid {
                    return Err(bad("malformed wildcard"));
                }
                out.push(Entry::Subdomains(format!(
                    ".{}",
                    domain.to_ascii_lowercase()
                )));
            } else {
                out.push(Entry::Exact(e.to_ascii_lowercase(), parse_ip(e)));
            }
        }
        Ok(Allowlist { entries: out })
    }

    /// True if the whole `value` is covered by an entry.
    ///
    /// The comparison ignores ASCII case, a trailing dot of a hostname for wildcards, and
    /// the spelling of an IP address. A wildcard `*.d` covers every value that ends in
    /// `.d`.
    ///
    /// ```
    /// use sagiri_core::config::Allowlist;
    ///
    /// let allow = Allowlist::new(&["2001:db8::/32".to_string(), "*.example.net".to_string()])?;
    /// assert!(allow.matches("2001:DB8:0:0::1"));
    /// assert!(allow.matches("api.example.net."));
    /// assert!(!allow.matches("example.net"));
    /// assert!(!Allowlist::empty().matches("api.example.net"));
    /// # Ok::<(), Box<dyn std::error::Error>>(())
    /// ```
    pub fn matches(&self, value: &str) -> bool {
        if self.entries.is_empty() {
            return false;
        }
        let lower = value.to_ascii_lowercase();
        let host = lower.strip_suffix('.').unwrap_or(&lower);
        let ip = parse_ip(value);
        self.entries.iter().any(|e| match e {
            Entry::Exact(s, eip) => *s == lower || (ip.is_some() && ip == *eip),
            Entry::Subdomains(suffix) => host.len() > suffix.len() && host.ends_with(suffix),
            Entry::Network(net, bits) => ip.is_some_and(|a| in_network(a, *net, *bits)),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    use std::fs;

    fn load_str(src: &str) -> Result<Config, ConfigError> {
        let dir = tempfile::TempDir::new().unwrap();
        let p = dir.path().join("config.toml");
        fs::write(&p, src).unwrap();
        Config::load(&p)
    }

    fn env_of(pairs: &[(&str, &str)]) -> impl Fn(&str) -> Option<OsString> + use<> {
        let m: HashMap<String, OsString> = pairs
            .iter()
            .map(|(k, v)| (k.to_string(), OsString::from(v)))
            .collect();
        move |k| m.get(k).cloned()
    }

    #[test]
    fn empty_file_gives_defaults() {
        assert_eq!(load_str("").unwrap(), Config::default());
        assert!(Config::default().rules.enabled(Kind::Phone));
    }

    #[test]
    fn missing_xdg_file_gives_none() {
        let dir = tempfile::TempDir::new().unwrap();
        let env = env_of(&[("XDG_CONFIG_HOME", dir.path().to_str().unwrap())]);
        assert_eq!(resolve_path(None, &env).unwrap(), None);
    }

    #[test]
    fn no_home_and_no_xdg_means_defaults() {
        assert_eq!(resolve_path(None, &env_of(&[])).unwrap(), None);
    }

    #[test]
    fn error_points_at_the_bad_key() {
        let msg = load_str("terms = [\"ok\"]\n[rules]\nterm = \"Secretco\"\nphone = 5")
            .unwrap_err()
            .to_string();
        assert!(
            msg.contains("line 4") && msg.contains("rules.phone"),
            "{msg}"
        );
        let msg = load_str("terms = [\"ok\"]\n[rules]\nphone = 5")
            .unwrap_err()
            .to_string();
        assert!(
            msg.contains("line 3") && msg.contains("rules.phone"),
            "{msg}"
        );
        assert!(!msg.contains("Secretco"));
    }

    #[test]
    fn example_parses() {
        let c = load_str(
            r#"
# Values that are never replaced.
allow = ["intranet.acme.com", "*.github.com", "10.20.0.0/16", "192.0.2.7"]

# Literal terms that are always replaced.
terms = ["Acme Corp", "Project Falcon"]

# Rules are on unless switched off here.
[rules]
phone = false
"#,
        )
        .unwrap();
        let expected = Config {
            allow: [
                "intranet.acme.com",
                "*.github.com",
                "10.20.0.0/16",
                "192.0.2.7",
            ]
            .map(String::from)
            .to_vec(),
            terms: ["Acme Corp", "Project Falcon"].map(String::from).to_vec(),
            rules: RuleToggles {
                phone: false,
                ..RuleToggles::default()
            },
            session: SessionConfig::default(),
        };
        assert_eq!(c, expected);
    }

    #[test]
    fn all_nine_toggles_are_accepted() {
        let src: String = std::iter::once("[rules]\n".to_string())
            .chain(Kind::ALL.iter().map(|k| format!("{} = false\n", k.id())))
            .collect();
        let c = load_str(&src).unwrap();
        for k in Kind::ALL {
            assert!(!c.rules.enabled(k), "{k:?}");
        }
    }

    #[test]
    fn structural_errors_name_the_key_and_hide_values() {
        let cases = [
            ("allows = [\"hush1\"]", "allows"),
            ("[rules]\nbogus = true", "bogus"),
            ("terms = \"hush2\"", "terms"),
            ("terms = [\"ok one\", 5]", "terms[1]"),
            ("[rules]\nphone = \"hush3\"", "rules.phone"),
            ("rules = \"hush4\"", "rules"),
        ];
        for (src, key) in cases {
            let msg = load_str(src).unwrap_err().to_string();
            assert!(msg.contains(key), "{msg}");
            assert!(msg.contains("config.toml"), "{msg}");
            assert!(msg.contains("line "), "{msg}");
            assert!(!msg.contains("hush"), "{msg}");
        }
    }

    #[test]
    fn syntax_error_has_position_and_no_term() {
        let msg = load_str("\nterms = [\"Hushed Corp\" \"x\"]")
            .unwrap_err()
            .to_string();
        assert!(msg.contains("line 2, column 24"), "{msg}");
        assert!(!msg.contains("Hushed"), "{msg}");
        let msg = load_str("terms = [\"Hushed\\q\"]").unwrap_err().to_string();
        assert!(!msg.contains("Hushed"), "{msg}");
    }

    #[test]
    fn resolve_path_order() {
        let dir = tempfile::TempDir::new().unwrap();
        let flag = dir.path().join("flag.toml");
        let envf = dir.path().join("env.toml");
        let xdg = dir.path().join("xdg");
        fs::create_dir_all(xdg.join("sagiri")).unwrap();
        let xdg_file = xdg.join("sagiri/config.toml");
        for f in [&flag, &envf, &xdg_file] {
            fs::write(f, "").unwrap();
        }
        let env = env_of(&[
            ("SAGIRI_CONFIG", envf.to_str().unwrap()),
            ("XDG_CONFIG_HOME", xdg.to_str().unwrap()),
            ("HOME", "/nonexistent"),
        ]);
        assert_eq!(resolve_path(Some(&flag), &env).unwrap(), Some(flag.clone()));
        assert_eq!(resolve_path(None, &env).unwrap(), Some(envf.clone()));
        let env = env_of(&[
            ("SAGIRI_CONFIG", ""),
            ("XDG_CONFIG_HOME", xdg.to_str().unwrap()),
        ]);
        assert_eq!(resolve_path(None, &env).unwrap(), Some(xdg_file));
    }

    #[test]
    fn explicit_missing_path_is_an_error_naming_it() {
        let env = env_of(&[("SAGIRI_CONFIG", "/nonexistent/sagiri.toml")]);
        let e = resolve_path(None, &env).unwrap_err();
        assert!(e.to_string().contains("/nonexistent/sagiri.toml"));
        let env = env_of(&[]);
        let e = resolve_path(Some(Path::new("/nonexistent/f.toml")), &env).unwrap_err();
        assert!(e.to_string().contains("/nonexistent/f.toml"));
    }

    #[test]
    fn xdg_unset_empty_or_relative_falls_back_to_home() {
        let home = tempfile::TempDir::new().unwrap();
        let dir = home.path().join(".config/sagiri");
        fs::create_dir_all(&dir).unwrap();
        let file = dir.join("config.toml");
        fs::write(&file, "").unwrap();
        let h = home.path().to_str().unwrap();
        for pairs in [
            vec![("HOME", h)],
            vec![("HOME", h), ("XDG_CONFIG_HOME", "")],
            vec![("HOME", h), ("XDG_CONFIG_HOME", "relative/dir")],
        ] {
            let env = env_of(&pairs);
            assert_eq!(resolve_path(None, &env).unwrap(), Some(file.clone()));
        }
    }

    #[test]
    fn allowlist_table() {
        let list = |e: &[&str]| {
            Allowlist::new(&e.iter().map(|s| s.to_string()).collect::<Vec<_>>()).unwrap()
        };
        let exact = list(&["Intranet.Acme.com", "10.0.0.1", "2001:db8::1"]);
        assert!(exact.matches("intranet.acme.com"));
        assert!(exact.matches("INTRANET.ACME.COM"));
        assert!(!exact.matches("acme.com"));
        assert!(exact.matches("010.0.0.1"));
        assert!(exact.matches("2001:DB8:0:0:0:0:0:1"));
        assert!(!exact.matches("10.0.0.2"));

        let wild = list(&["*.github.com"]);
        assert!(wild.matches("api.github.com"));
        assert!(wild.matches("a.b.github.com"));
        assert!(wild.matches("API.GitHub.com."));
        assert!(!wild.matches("github.com"));
        assert!(!wild.matches("notgithub.com"));

        let net = list(&["10.20.0.0/16", "2001:db8::/32", "0.0.0.0/0"]);
        assert!(net.matches("10.20.3.4"));
        assert!(net.matches("10.21.0.1")); // inside 0.0.0.0/0
        assert!(net.matches("2001:db8:1::5"));
        assert!(!net.matches("2001:db9::1"));
        let net = list(&["10.20.0.0/16", "2001:db8::/32"]);
        assert!(!net.matches("10.21.0.1"));
        assert!(!net.matches("example.com"));
        assert!(!Allowlist::empty().matches("10.0.0.1"));
    }

    #[test]
    fn invalid_allow_entries_are_rejected_with_index() {
        for bad in [
            "",
            "  ",
            "*.",
            "*..x",
            "10.0.0.0/33",
            "10.0.0.0/",
            "x/16",
            "*.a b",
        ] {
            let e = Allowlist::new(&["ok.example.com".to_string(), bad.to_string()])
                .map(|_| ())
                .unwrap_err();
            assert!(e.to_string().contains("allow[1]"), "{bad:?}: {e}");
        }
        let e = load_str("allow = [\"*.\"]").unwrap_err().to_string();
        assert!(e.contains("allow[0]") && e.contains("config.toml"), "{e}");
    }

    #[test]
    fn invalid_terms_are_rejected_with_index() {
        let e = load_str("terms = [\"fine\", \"x\"]")
            .unwrap_err()
            .to_string();
        assert!(e.contains("terms[1]") && e.contains("config.toml"), "{e}");
        let e = load_str("terms = [\"a\\nb\"]").unwrap_err().to_string();
        assert!(e.contains("terms[0]"), "{e}");
    }

    #[test]
    fn overlong_term_is_rejected_on_load() {
        let src = format!("terms = [\"{}\"]\n", "a".repeat(1001));
        let e = load_str(&src).unwrap_err().to_string();
        assert!(e.contains("config.toml"), "{e}");
        assert!(e.contains("terms[0]"), "{e}");
        assert!(e.contains("term is longer than 1000 characters"), "{e}");
    }

    #[test]
    fn session_defaults() {
        let c = load_str("").unwrap();
        assert_eq!(c.session.hotkey, None);
        assert_eq!(c.session.primary, PrimaryMode::Off);
        assert!(c.session.notify);
        assert_eq!(c.session, SessionConfig::default());
    }

    #[test]
    fn session_section_parses() {
        let c = load_str(
            r#"
[session]
hotkey = "ctrl+alt+o"
primary = "mirror"
notify = true
"#,
        )
        .unwrap();
        let expected = SessionConfig {
            hotkey: Some("ctrl+alt+o".to_string()),
            primary: PrimaryMode::Mirror,
            notify: true,
        };
        assert_eq!(c.session, expected);
        let c = load_str("[session]\nnotify = false\nhotkey = \"not checked here\"").unwrap();
        assert!(!c.session.notify);
        assert_eq!(c.session.hotkey.as_deref(), Some("not checked here"));
        for src in ["session = { notify = false }", "session.notify = false"] {
            assert!(!load_str(src).unwrap().session.notify, "{src}");
        }
    }

    #[test]
    fn session_primary_accepts_all_three_words() {
        for (word, mode) in [
            ("off", PrimaryMode::Off),
            ("mirror", PrimaryMode::Mirror),
            ("watch", PrimaryMode::Watch),
        ] {
            let c = load_str(&format!("[session]\nprimary = \"{word}\"")).unwrap();
            assert_eq!(c.session.primary, mode, "{word}");
        }
    }

    #[test]
    fn unknown_session_key_is_error() {
        let msg = load_str("[session]\nnotify = true\nbogus = true")
            .unwrap_err()
            .to_string();
        assert!(msg.contains("bogus"), "{msg}");
        assert!(msg.contains("config.toml"), "{msg}");
        assert!(msg.contains("line 3, column 1"), "{msg}");
    }

    #[test]
    fn invalid_primary_value_is_error() {
        for src in ["[session]\nprimary = \"on\"", "[session]\nprimary = 1"] {
            let msg = load_str(src).unwrap_err().to_string();
            assert!(
                msg.contains(r#"`session.primary`: expected "off", "mirror" or "watch""#),
                "{msg}"
            );
            assert!(msg.contains("line 2"), "{msg}");
            // `on` itself occurs in "session" and "column"; the quoted value must not.
            assert!(!msg.contains("\"on\"") && !msg.contains("`on`"), "{msg}");
        }
        let msg = load_str("[session]\nprimary = \"hush\"")
            .unwrap_err()
            .to_string();
        assert!(!msg.contains("hush"), "{msg}");
    }

    #[test]
    fn wrong_type_for_hotkey_and_notify_is_error() {
        let cases = [
            (
                "[session]\nhotkey = 5",
                "`session.hotkey`: expected a string",
            ),
            (
                "[session]\nnotify = \"hush\"",
                "`session.notify`: expected a boolean",
            ),
        ];
        for (src, expected) in cases {
            let msg = load_str(src).unwrap_err().to_string();
            assert!(msg.contains(expected), "{msg}");
            assert!(msg.contains("config.toml: line 2"), "{msg}");
            assert!(!msg.contains("hush"), "{msg}");
        }
    }

    #[test]
    fn session_that_is_not_a_table_is_error() {
        for src in [
            "session = \"hush\"",
            "session = []",
            "session = [\"hush\", \"watch\", false]",
            "session = [\"hush\", \"watch\", false, 1]",
            "session = true",
        ] {
            let msg = load_str(src).unwrap_err().to_string();
            assert!(msg.contains("`session`: expected a table"), "{msg}");
            assert!(msg.contains("config.toml: line 1, column 11"), "{msg}");
            assert!(!msg.contains("hush"), "{msg}");
        }
        let msg = load_str("[[session]]\nhotkey = \"hush\"")
            .unwrap_err()
            .to_string();
        assert!(msg.contains("`session`: expected a table"), "{msg}");
        assert!(msg.contains("config.toml: line 1"), "{msg}");
        assert!(!msg.contains("hush"), "{msg}");
    }

    fn assert_rules_not_a_table(src: &str, position: &str) {
        let msg = load_str(src).unwrap_err().to_string();
        assert!(msg.contains("`rules`: expected a table"), "{src}: {msg}");
        assert!(msg.contains(position), "{src}: {msg}");
        assert!(!msg.contains("hush"), "{src}: {msg}");
    }

    #[test]
    fn rules_array_is_error() {
        let nine = format!("rules = [{}false]", "true, ".repeat(8));
        let ten = format!("rules = [{}]", ["true"; 10].join(", "));
        for src in [
            "rules = [false]",
            "rules = []",
            "rules = [false, false]",
            nine.as_str(),
            ten.as_str(),
            "rules = [false, \"hush\"]",
            "rules = [[false]]",
        ] {
            assert_rules_not_a_table(src, "config.toml: line 1, column 9");
        }
    }

    #[test]
    fn rules_scalar_is_error() {
        for src in ["rules = \"hush\"", "rules = true", "rules = 1"] {
            assert_rules_not_a_table(src, "config.toml: line 1, column 9");
        }
    }

    #[test]
    fn rules_array_of_tables_is_error() {
        assert_rules_not_a_table("[[rules]]\nterm = false", "config.toml: line 1");
    }

    #[test]
    fn rules_error_position_is_the_value() {
        let src = "allow = [\"*.example.com\"]\nterms = [\"Acme Corp\"]\nrules = [false]";
        assert_rules_not_a_table(src, "line 3, column 9");
    }

    #[test]
    fn rules_table_forms_parse() {
        let term_off = RuleToggles {
            term: false,
            ..RuleToggles::default()
        };
        for src in [
            "[rules]\nterm = false",
            "rules = { term = false }",
            "rules.term = false",
        ] {
            assert_eq!(load_str(src).unwrap().rules, term_off, "{src}");
        }
        for src in ["[rules]", "rules = {}"] {
            assert_eq!(
                load_str(src).unwrap().rules,
                RuleToggles::default(),
                "{src}"
            );
        }
    }
}
