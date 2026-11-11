# Sagiri tickets

Sagiri obfuscates pasted text. It finds sensitive values (IP addresses, URLs, hostnames,
email addresses, card numbers, IBANs, phone numbers, user-defined terms) and replaces each
with a realistic-looking fake value. The same original always gets the same replacement,
across pastes and across sessions.

Read this file before starting any ticket. It holds everything the tickets share.

## How tickets work

- One file per ticket: `NNNN-slug.md`. The id is the four digits.
- Front matter: `id`, `title`, `status` (`todo` | `in-progress` | `done`), `depends_on`.
- A ticket may start once every ticket in its `depends_on` is merged into `master`.
- Work happens on a branch named `ticket/NNNN`, cut from `master` (the default branch).
- The implementer builds exactly what **Scope** lists, nothing from **Out of scope**, and
  sets `status: done` in the final commit.
- The reviewer checks the branch against **Acceptance criteria** and the definition of done
  below.
- If a ticket contradicts this README, or something pinned here cannot work, stop and report
  it. Do not guess, and do not edit other tickets.

## Index

| ID | Title | Depends on |
|---|---|---|
| [0001](0001-project-scaffolding.md) | Project scaffolding | – |
| [0002](0002-pseudonymization-primitives.md) | Keyed pseudonymization primitives and key file | 0001 |
| [0003](0003-detection-engine.md) | Detection and replacement engine | 0002 |
| [0004](0004-ip-address-rule.md) | IP address rules (IPv4 + IPv6) | 0003 |
| [0005](0005-filter-mode.md) | Filter mode: stdin to stdout | 0004 |
| [0006](0006-hostname-url-rules.md) | Hostname and URL rules | 0003, 0004 |
| [0007](0007-email-rule.md) | Email address rule | 0006 |
| [0008](0008-payment-card-iban-rules.md) | Payment card and IBAN rules | 0003 |
| [0009](0009-phone-number-rule.md) | Phone number rule | 0003 |
| [0010](0010-tui-paste-obfuscate-copy.md) | TUI: paste, obfuscate, copy | 0005 |
| [0011](0011-tui-findings-review.md) | TUI: findings review | 0010 |
| [0012](0012-config-file.md) | Config file | 0005, 0007 |
| [0013](0013-ci-workflow.md) | CI workflow | 0001 |

Order: 0001 → 0002 → 0003 are strictly sequential. After 0003, the tickets 0004, 0008 and
0009 are independent of each other. After 0004, the tickets 0005 and 0006 are independent.
Parallel rule tickets each add one line to `default_rules()`; expect a trivial merge conflict
there.

## Definition of done (every ticket)

- `scripts/check.sh` passes (format, clippy with warnings as errors, all tests).
- New behaviour has tests; every acceptance criterion maps to a named test or a command.
- No pasted text, original value or key material is logged, printed in an error message or
  written to disk. Error messages name files, keys and positions, never content.
- Existing golden vectors are unchanged, unless the ticket says otherwise.
- No new `unsafe`. No dependency added that the ticket does not need.
- Test data is fictional: made-up names, documentation IP ranges, published test card
  numbers.

## Product decisions

- **Interface:** a terminal UI is the primary interface. A stdin-to-stdout filter mode exists
  for scripting and for end-to-end tests.
- **One-way.** Replacements are derived from a secret key. Sagiri never stores original
  values and has no restore feature.
- **Local only.** No network access at run time.
- **Unix only for the MVP** (Linux, macOS). Windows is not a target.

## Architecture

### Layout

```
Cargo.toml                    workspace
crates/sagiri-core/           library: detection and pseudonymization, no terminal I/O
  src/paths.rs                0002  XDG paths
  src/key.rs                  0002  secret key, key file
  src/pseudo.rs               0002  Pseudonymizer: stream, permute, mimic
  src/rule.rs                 0003  Kind, Rule, Context
  src/engine.rs               0003  Engine, Finding, render
  src/rules/mod.rs            0003  default_rules()
  src/rules/ip.rs             0004
  src/rules/hostname.rs       0006
  src/rules/url.rs            0006
  src/rules/email.rs          0007
  src/rules/card.rs           0008
  src/rules/iban.rs           0008
  src/rules/phone.rs          0009
  src/rules/term.rs           0012
  src/config.rs               0012
crates/sagiri/                binary `sagiri`
  src/main.rs                 0001  clap; 0005 mode dispatch
  src/setup.rs                0005  build_engine()
  src/filter.rs               0005
  src/tui/                    0010, 0011
scripts/check.sh              0001  the quality gate
```

### Core API (pinned; tickets 0002 and 0003 implement it)

```rust
// key.rs
pub struct Key(/* 32 secret bytes, zeroized on drop */);

// pseudo.rs
pub struct Pseudonymizer { /* .. */ }
impl Pseudonymizer {
    pub fn new(key: &Key) -> Self;
    /// Deterministic byte stream for (domain, input).
    pub fn stream(&self, domain: &str, input: &[u8]) -> DetStream;
    /// Keyed bijection on 0..2^bits.
    pub fn permute(&self, domain: &str, bits: u32, value: u64) -> u64;
    /// Keyed bijection on the subset of 0..2^bits for which `member` is true.
    pub fn permute_within(&self, domain: &str, bits: u32, value: u64,
                          member: impl Fn(u64) -> bool) -> u64;
    /// Pseudo-text with the same shape as `shape`, seeded by (domain, seed).
    pub fn mimic(&self, domain: &str, seed: &[u8], shape: &str) -> String;
}
pub struct DetStream { /* .. */ }
impl DetStream {
    pub fn fill(&mut self, buf: &mut [u8]);
    pub fn next_u64(&mut self) -> u64;
    pub fn below(&mut self, n: u64) -> u64;            // uniform in 0..n
    pub fn pick<'a, T>(&mut self, items: &'a [T]) -> &'a T;
}

// rule.rs
pub enum Kind { Term, Url, Email, Ipv4, Ipv6, Hostname, PaymentCard, Iban, Phone }
pub trait Rule: Send + Sync {
    fn kind(&self) -> Kind;
    /// Byte ranges in `text` that this rule wants to replace. Already validated.
    fn detect(&self, text: &str) -> Vec<std::ops::Range<usize>>;
    /// Replacement for one matched string.
    fn replace(&self, original: &str, ctx: &Context<'_>) -> String;
}
pub struct Context<'a> { /* .. */ }
impl Context<'_> {
    pub fn pseudo(&self) -> &Pseudonymizer;
    /// Runs the whole engine over a fragment of the matched string.
    pub fn nested(&self, fragment: &str) -> String;
    /// True if the user's allowlist covers `value`. Always false until 0012.
    pub fn is_allowed(&self, value: &str) -> bool;
}

// engine.rs
pub struct Finding {
    pub span: std::ops::Range<usize>,   // in the input text
    pub kind: Kind,
    pub original: String,
    pub replacement: String,
}
pub struct Rendered { pub text: String, pub spans: Vec<RenderedSpan> }
pub struct RenderedSpan {
    pub range: std::ops::Range<usize>,  // in Rendered::text
    pub finding: usize,                 // index into the findings slice
    pub replaced: bool,                 // false if the filter kept the original
}
pub struct Engine { /* .. */ }
impl Engine {
    pub fn new(pseudo: Pseudonymizer, rules: Vec<Box<dyn Rule>>) -> Self;
    pub fn context(&self) -> Context<'_>;
    pub fn analyze(&self, text: &str) -> Vec<Finding>;
    pub fn obfuscate(&self, text: &str) -> String;
}
pub fn render(text: &str, findings: &[Finding],
              include: impl Fn(&Finding) -> bool) -> Rendered;

// rules/mod.rs
pub fn default_rules() -> Vec<Box<dyn Rule>>;
```

`default_rules()` returns the rules in this order, which is also the order of `Kind`:
term, url, email, ipv4, ipv6, hostname, payment_card, iban, phone. A ticket adds its rule at
its position in that order.

### How consistency works

- **Derivation, not storage.** A replacement is a pure function of
  (key, domain label, canonical form of the original). Nothing is remembered between runs
  except the key.
- **Canonical form.** Each rule defines one: the parsed address for IPs, lowercase for
  hostnames and emails, digits only for cards and phone numbers. Two spellings of the same
  value map to the same replacement. The original's formatting (case, separators, grouping)
  is re-applied to the output.
- **Composite values delegate.** A URL's host goes through the hostname or IP mapping, an
  email's domain through the hostname mapping. That is what keeps `git.acme.com` identical
  whether it appears bare, in a URL or in an email address.
- **Stability contract.** The same key must give the same replacements in every future
  version. Therefore:
  - no `rand` crate anywhere in the derivation path (its algorithms may change between
    versions); all randomness comes from `Pseudonymizer`;
  - every derivation is domain-separated under the scheme label `sagiri/v1`;
  - every rule has a golden-vector test: fixed test key, literal inputs, literal expected
    outputs. Changing an existing vector is a breaking change and needs its own ticket.
- **Test key.** Golden vectors use the key whose bytes are `0x00, 0x01, … 0x1f`.

### Rules for writing rules

- Validate before reporting: checksums, numeric ranges, known prefixes. A false positive
  mangles the user's text.
- The `regex` crate has no look-around. Match the candidate with a regex, then check the
  neighbouring characters in code.
- A match never contains `\n` or `\r`. A replacement never contains them either. The number
  of lines in the output always equals the number in the input.
- Return a range from `detect` only if the value should be replaced. If `replace` returns
  the original unchanged, the engine drops the finding.
- Compile regexes once (`std::sync::LazyLock`).

### Files Sagiri touches

Paths follow the XDG Base Directory specification, resolved in `paths.rs` only.

| File | Path | Default when the variable is unset |
|---|---|---|
| Config | `$XDG_CONFIG_HOME/sagiri/config.toml` | `~/.config/sagiri/config.toml` |
| Key | `$XDG_DATA_HOME/sagiri/key` | `~/.local/share/sagiri/key` |

- An unset, empty or relative `XDG_*` value is ignored and the default is used.
- The layout is the same on Linux and macOS.
- An explicit flag wins over a `SAGIRI_*` environment variable, which wins over the XDG path.
- The key is kept out of the config directory on purpose: config directories often end up in
  dotfiles repositories.

### Threat model in one paragraph

Without the key, a replacement reveals nothing about the original beyond its kind and shape.
With the key, small value spaces such as IPv4 can be enumerated, so the key file must stay
private. Losing or replacing the key changes every replacement.

## Crates (versions seen on crates.io on 2026-11-11)

Add dependencies with `cargo add` and read the current docs on docs.rs; several of these
changed their API recently.

| Purpose | Crate | Note |
|---|---|---|
| CLI | `clap` 4.6 | derive feature |
| Keyed hashing | `hmac` 0.13, `sha2` 0.11 | new major versions of the RustCrypto traits |
| Key handling | `zeroize` 1.9, `getrandom` | |
| Regex | `regex` 1.13 | no look-around, no backreferences |
| Public suffix list | `psl` 2.1 | list is compiled in |
| TUI | `ratatui` 0.30, `crossterm` 0.29 | `tui-textarea` 0.7 needs ratatui 0.29: do not use it |
| Clipboard | `arboard` 3.7 | enable `wayland-data-control` |
| Config | `toml` 0.9, `serde` 1 | |
| Errors | `thiserror` 2 (core), `anyhow` 1 (binary) | |
| Tests | `proptest` 1.12, `assert_cmd` 2.2, `tempfile` | |

## Backlog (not ticketed)

- MAC addresses
- Secrets: API keys, tokens, JWTs, private keys
- National-format phone numbers with a configurable default region
- National identifiers (AHV, SSN, …)
- Person names: dictionary first, NER as a spike
- Prefix-preserving IP mode and CIDR-aware mapping
- Optional obfuscation of URL paths
- Profiles (several keys) and a key rotation command
- Auto-copy on paste
- OSC 52 clipboard for SSH sessions
- File arguments (`sagiri file.log`)
- UUIDs and cloud account identifiers
- Release packaging
