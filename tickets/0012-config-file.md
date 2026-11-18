---
id: "0012"
title: Config file
status: done
depends_on: ["0005", "0007"]
---

# 0012 — Config file

## Why

Pattern-based detection cannot know that "Acme Corp" or "Project Falcon" is sensitive, that
`*.github.com` is harmless, or that a user never wants phone numbers touched. A small config
file lets the user teach Sagiri these things once.

## Scope

### File and location

- Format: TOML.
- Location, first match wins:
  1. `--config <PATH>` (new global option)
  2. `SAGIRI_CONFIG`, if set and non-empty
  3. `$XDG_CONFIG_HOME/sagiri/config.toml`, default `~/.config/sagiri/config.toml`, through
     `sagiri_core::paths::config_dir()`
- A path given by flag or environment variable must exist; otherwise it is an error. A
  missing file at the XDG path means "all defaults" and is not an error.
- Example, also to be included in `README.md`:

  ```toml
  # Values that are never replaced.
  allow = ["intranet.acme.com", "*.github.com", "10.20.0.0/16", "192.0.2.7"]

  # Literal terms that are always replaced.
  terms = ["Acme Corp", "Project Falcon"]

  # Rules are on unless switched off here.
  [rules]
  phone = false
  ```

### `sagiri-core/src/config.rs`

- `pub struct Config { pub allow: Vec<String>, pub terms: Vec<String>, pub rules: RuleToggles }`
  with serde, `deny_unknown_fields` on every level, and defaults for everything.
- `RuleToggles`: one `bool` per `Kind::id()` (`term`, `url`, `email`, `ipv4`, `ipv6`,
  `hostname`, `payment_card`, `iban`, `phone`), default `true`.
- `Config::load(path: &Path) -> Result<Config, ConfigError>`.
- `resolve_path(flag: Option<&Path>, env: &dyn Fn(&str) -> Option<OsString>)
  -> Result<Option<PathBuf>, ConfigError>`: `None` means "no file, use defaults".
- **Error messages** name the file, the key or array index, and the line and column. They
  never contain a value from the file. For TOML syntax errors use the error's message and
  span to build the text; its `Display` output includes the offending source line.

### Allowlist

- `pub struct Allowlist` built from `Config::allow`. Each entry is one of:
  - **network**: contains `/` and parses as an IPv4 or IPv6 network in CIDR notation;
  - **subdomain wildcard**: starts with `*.` followed by a hostname;
  - **exact**: anything else.
- `Allowlist::matches(&self, value: &str) -> bool`:
  - exact: equal ignoring ASCII case; if both sides parse as IP addresses, equal as
    addresses;
  - subdomain wildcard `*.d`: the value, lower-cased and without a trailing dot, ends with
    `.d`. It does not match `d` itself;
  - network: the value parses as an IP address inside the network.
- Invalid entries (empty, a bare `*.`, a malformed network) are a `ConfigError` naming
  `allow[index]`.
- Wiring:
  - `Engine::with_allowlist(self, allow: Allowlist) -> Self`.
  - `Engine::analyze` skips a candidate whose original matches.
  - `Context::is_allowed` returns `matches(value)`.

### Terms (`rules/term.rs`)

- `TermRule::new(terms: &[String])`, kind `Kind::Term`, first in `default_rules()` order.
  It is added only when there is at least one term.
- A term is matched literally, ignoring case, as a whole word: the characters before and
  after the match are not alphanumeric. Where terms overlap, the longest wins.
- Invalid terms are a `ConfigError` naming `terms[index]`: fewer than two characters after
  trimming, or containing a line break.
- Replacement: `ctx.pseudo().mimic("term", seed, matched)` with `seed` = the matched text in
  lower case. `Acme Corp` becomes two capitalized pseudo-words of four letters each.

### Binary

- `setup::Options` gains `config: Option<PathBuf>`.
- `setup::build_engine` loads the config, keeps only the rules that are switched on, adds
  the term rule, and sets the allowlist. Filter mode and the TUI both get this through
  `build_engine`; neither reads the config itself.
- Config errors: `sagiri: <message>`, exit 1.

### Documentation

- `README.md`: a "Configuration" section with the location rules, the example, and this
  warning: the config file contains the user's sensitive terms in plain text, so it should
  not be shared or committed.

## Out of scope

- Custom regular-expression rules.
- A kind per term (person, organization) with matching generators (backlog: person names).
- Making the suffix lists and `AMBIGUOUS_TLDS` of ticket 0006 configurable.
- Options for URL path obfuscation or IP mapping modes (backlog).
- Reloading the config while the TUI runs; writing the config from the TUI.

## Design notes

- **XDG.** The config path follows the XDG Base Directory specification via the `paths`
  module from ticket 0002. Do not resolve it anywhere else.
- **The allowlist is checked on whole values**, at two points: on a finding's original
  text, and inside the hostname mapping. Allowing `github.com` therefore keeps
  `https://github.com/org/repo` and `git@github.com`'s domain, but not `api.github.com`;
  that needs `*.github.com`.
- **An allowed domain does not make an email address safe.** In `jane@intranet.acme.com`
  the domain is kept and the local part is still replaced. To keep a whole address, list
  the address.
- **Terms are the one place where sensitive words are written down**, by the user, on
  purpose. This does not conflict with "Sagiri stores no originals": Sagiri never writes
  this file.
- **Unknown keys are errors** so that a typo such as `allows = [...]` does not silently
  disable protection the user thinks is in place.

## Acceptance criteria

Core tests:

- [ ] An empty file and a missing XDG file both give the default config.
- [ ] The example above parses into the expected `Config`.
- [ ] Unknown top-level key, unknown key in `[rules]`, wrong type (`terms = "x"`): each is
      an error naming the key; the message contains no value from the file.
- [ ] A TOML syntax error on a line containing a term gives a message with line and column
      and without the term.
- [ ] `resolve_path` order: flag, `SAGIRI_CONFIG`, XDG path; `XDG_CONFIG_HOME` unset,
      empty and relative fall back to `~/.config`. No test calls `std::env::set_var`.
- [ ] Allowlist table: exact (case-insensitive; `010.0.0.1` equals `10.0.0.1`), wildcard
      (`*.github.com` matches `api.github.com` and `a.b.github.com`, not `github.com`, not
      `notgithub.com`), network (`10.20.0.0/16` matches `10.20.3.4`, not `10.21.0.1`; an
      IPv6 network works).
- [ ] Invalid allow entries and invalid terms are rejected with their index.
- [ ] Engine with `allow = ["10.20.0.0/16", "*.github.com", "intranet.acme.com"]`:
      `10.20.3.4`, `api.github.com`, `https://api.github.com/x` and
      `http://10.20.3.4:8080/` produce no finding; in `jane@intranet.acme.com` only `jane`
      changes; `10.21.0.1` and `acme.com` are still replaced.
- [ ] Terms: `Acme Corp`, `acme corp` and `ACME CORP` are all replaced, with the same
      letters and each in its own case; `AcmeCorp` and `Acme Corporation` are not matched
      by the term `Acme Corp`; with terms `Acme` and `Acme Corp`, the text `Acme Corp` is
      one finding.
- [ ] Rule toggles: with `ipv4 = false` an IPv4 address is left alone and hostnames are
      still replaced; with `term = false` terms are left alone. Every one of the nine keys
      is accepted even if its rule is not implemented yet.
- [ ] Golden vectors with the test key: three terms, as literals.

End-to-end tests (`assert_cmd`, temporary directories for key and config):

- [ ] `--config` is used; `SAGIRI_CONFIG` is used; the flag wins.
- [ ] `--config` pointing to a missing file exits 1 and names the path.
- [ ] With only `XDG_CONFIG_HOME` set on the child process, the file at
      `$XDG_CONFIG_HOME/sagiri/config.toml` is used.
- [ ] A config with an unknown key exits 1; stdout is empty.

And:

- [ ] `README.md` has the "Configuration" section described above.
- [ ] `scripts/check.sh` passes.

## Test plan

- Unit tests in `config.rs` and `rules/term.rs`; engine-level tests in
  `crates/sagiri-core/tests/config_engine.rs`; golden vectors in
  `crates/sagiri-core/tests/golden_term.rs`.
- End-to-end tests in `crates/sagiri/tests/config.rs`.
