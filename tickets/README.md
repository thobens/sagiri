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
- `/ticket NNNN` runs a ticket end to end: implementer, reviewer, up to two fix rounds, then
  an offer to merge into `master`. It is defined in `.claude/skills/ticket/SKILL.md`; the
  two agents are defined in `.claude/agents/`.

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
| [0014](0014-library-target-shared-clipboard.md) | Library target and shared clipboard module | – |
| [0015](0015-selection-aware-clipboard.md) | Selection-aware clipboard | 0014 |
| [0016](0016-cli-selectable-input.md) | `sagiri-cli`: selectable input | 0015 |
| [0017](0017-cli-clipboard-output.md) | `sagiri-cli`: clipboard output | 0016 |
| [0018](0018-session-settings-runtime-dir.md) | Session settings and runtime directory | 0014 |
| [0019](0019-session-core.md) | Session core | 0015 |
| [0020](0020-session-control-channel.md) | Control channel and `sagiri-session` | 0018, 0019 |
| [0021](0021-x11-clipboard-watcher.md) | X11 clipboard watcher | 0020 |
| [0022](0022-desktop-notifications.md) | Desktop notifications | 0020 |
| [0023](0023-x11-global-hotkey.md) | X11 global hotkey | 0021 |
| [0024](0024-primary-mirror.md) | PRIMARY mirror | 0020 |
| [0025](0025-primary-watch.md) | PRIMARY watch (experimental) | 0021, 0024 |
| [0026](0026-xvfb-integration-script.md) | Xvfb integration script | 0023 |
| [0027](0027-makefile.md) | Makefile | 0016, 0020 |
| [0028](0028-license.md) | License | – |
| [0029](0029-pkgbuild.md) | PKGBUILD | 0027, 0028 |
| [0030](0030-rules-not-a-table.md) | Reject a `rules` value that is not a table | 0018 |
| [0031](0031-image-owner-answers-text-request.md) | Leave an image alone when its owner answers a text request with other data | 0021 |
| [0032](0032-flaky-stale-socket-test.md) | Make `stale_socket_is_replaced` independent of tests that start processes | 0022 |

Order of the MVP tickets: 0001 → 0002 → 0003 are strictly sequential. After 0003, the
tickets 0004, 0008 and 0009 are independent of each other. After 0004, the tickets 0005 and
0006 are independent. Parallel rule tickets each add one line to `default_rules()`; expect a
trivial merge conflict there.

Order of the tickets from 0014 on:

- 0014 comes first. After it, 0015, 0018 and 0028 are independent of each other.
- After 0015, the tickets 0016 and 0019 are independent. 0017 follows 0016.
- 0020 needs 0018 and 0019.
- After 0020, the tickets 0021, 0022, 0024 and 0027 are independent.
- 0023, 0025 and 0026 run one after another: all three build on `session/x11.rs`.
- 0029 comes last of the tickets 0014 to 0029.
- 0030 needs only 0018 and is independent of every other ticket. It may run at any point
  after 0018.
- 0031 and 0032 are bug tickets found while running 0021 and 0022. They are independent of
  each other and of the tickets 0023 to 0029. 0031 can use the script of 0026 for its
  reproduction if that ticket is merged first.
- Milestones: the pure CLI is complete after 0017. Session mode works with a window-manager
  binding after 0021 and with its own hotkey after 0023.
- Expected trivial merge conflicts: module lines in `crates/sagiri/src/lib.rs`,
  `Cargo.toml` and `Cargo.lock`, and the user-facing `README.md`.

This file already describes the state after all of these tickets (layout, pinned API, files,
crates). A ticket from 0014 on edits the user-facing `README.md` and `CLAUDE.md` where its
scope says so, and leaves this file alone.

## Definition of done (every ticket)

- `scripts/check.sh` passes (format, clippy with warnings as errors, all tests).
- New behaviour has tests; every acceptance criterion maps to a named test or a command.
- No pasted text, original value or key material is logged, printed in an error message or
  written to disk. Error messages name files, keys and positions, never content.
- Existing golden vectors are unchanged, unless the ticket says otherwise.
- No new `unsafe`. No dependency added that the ticket does not need.
- Test data is fictional: made-up names, documentation IP ranges, published test card
  numbers.
- Nothing that `scripts/check.sh` runs touches a real clipboard, a display server or a
  notification daemon. CI has none of them.

## Product decisions

- **Three binaries, one package.** They share the library target of `crates/sagiri` and the
  same key and config.
  - `sagiri`: the terminal UI, the primary interface. With a pipe on stdin it is a
    stdin-to-stdout filter, used for scripting and for end-to-end tests.
  - `sagiri-cli`: non-interactive. Input from stdin or a clipboard selection, output to
    stdout or a clipboard selection.
  - `sagiri-session`: session mode. A long-running process that, while a session is active,
    replaces copied text on the clipboard with its obfuscated form.
- **One-way.** Replacements are derived from a secret key. Sagiri never stores original
  values and has no restore feature.
- **Local only.** No network access at run time.
- **Unix only** (Linux, macOS). Windows is not a target.
- **Session mode is Linux with X11 only.** On Wayland and macOS `sagiri-session run` exits
  with a message saying so. `sagiri` and `sagiri-cli` work wherever the clipboard crate
  does.
- **Session mode rules**, the same for every way of starting a session:
  - Text is always written back as plain text. Rich formats such as HTML are dropped,
    because they can carry originals (link targets, for example).
  - File lists, images and other content without text are left untouched.
  - Starting a session also processes what is on the clipboard at that moment.
  - Fail closed: if the obfuscated text cannot be written, the selection is cleared and the
    user is told.
  - Between a copy and its replacement the original is on the clipboard for a short time,
    and a clipboard manager may record it. Session mode prevents accidental pasting. It is
    not a boundary against other software on the same machine. The user-facing README says
    this in plain words.

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
  src/config.rs               0012  0018 adds the [session] table
crates/sagiri/                library `sagiri` and three binaries
  src/lib.rs                  0014  the modules below, shared by all binaries
  src/main.rs                 0001  binary `sagiri`: clap; 0005 mode dispatch
  src/bin/sagiri-cli.rs       0016  binary `sagiri-cli`
  src/bin/sagiri-session.rs   0020  binary `sagiri-session`
  src/setup.rs                0005  build_engine(); 0018 load()
  src/filter.rs               0005
  src/clipboard.rs            0014  moved out of tui/; 0015 selections
  src/cli.rs                  0016, 0017  logic of `sagiri-cli`
  src/holder.rs               0017  keeps clipboard content alive after exit
  src/tui/                    0010, 0011
  src/session/mod.rs          0019
  src/session/core.rs         0019  Session, Event, run_loop; 0024 mirror
  src/session/notify.rs       0019  Notice, Notifier; 0020 stderr; 0022 notify-send
  src/session/control.rs      0020  socket protocol, single instance
  src/session/daemon.rs       0020  wires session, control, signals, backend
  src/session/backend.rs      0020  seam; 0021 detect()
  src/session/x11.rs          0021  watcher thread; 0023 key grab; 0025 PRIMARY
  src/session/hotkey.rs       0023  hotkey parser and pure helpers
  src/session/settle.rs       0025  PRIMARY settle state machine
scripts/check.sh              0001  the quality gate
scripts/x11-test.sh           0026  opt-in tests against a real X server (Xvfb)
Makefile                      0027
LICENSE-MIT, LICENSE-APACHE   0028
packaging/arch/PKGBUILD       0029
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

### Binary crate API (pinned; tickets 0014 to 0025 implement it)

Tickets from 0014 on build on each other through these items. The ticket named in a comment
introduces the item; later tickets use it as written here. Private helpers are free.

```rust
// crates/sagiri/src/clipboard.rs
pub trait Clipboard {                                  // 0010; used by the TUI, unchanged
    fn get_text(&mut self) -> anyhow::Result<String>;
    fn set_text(&mut self, text: &str) -> anyhow::Result<()>;
}
pub enum Selection { Clipboard, Primary }              // 0015
pub enum Content { Text(String), Files, Other }        // 0015
pub trait SelectionClipboard {                         // 0015
    /// What the selection holds right now.
    fn read(&mut self, selection: Selection) -> anyhow::Result<Content>;
    fn write_text(&mut self, selection: Selection, text: &str) -> anyhow::Result<()>;
    fn clear(&mut self, selection: Selection) -> anyhow::Result<()>;
}
pub struct SystemClipboard { /* .. */ }                // implements both traits

// crates/sagiri/src/setup.rs
pub struct Options { pub key_file: Option<PathBuf>, pub config: Option<PathBuf> }
pub struct Loaded { pub engine: Engine, pub config: Config }       // 0018
pub fn load(opts: &Options) -> anyhow::Result<Loaded>;             // 0018
pub fn build_engine(opts: &Options) -> anyhow::Result<Engine>;     // 0005

// crates/sagiri-core/src/config.rs                    // 0018
pub struct SessionConfig {
    pub hotkey: Option<String>,   // raw text, parsed by ticket 0023
    pub primary: PrimaryMode,     // default Off
    pub notify: bool,             // default true
}
pub enum PrimaryMode { Off, Mirror, Watch }            // "off" | "mirror" | "watch"
// `Config` gains `pub session: SessionConfig`.

// crates/sagiri-core/src/paths.rs                     // 0018
pub fn runtime_dir() -> Result<PathBuf, PathError>;    // $XDG_RUNTIME_DIR/sagiri
pub fn runtime_dir_with(env: EnvLookup<'_>) -> Result<PathBuf, PathError>;

// crates/sagiri/src/session/notify.rs                 // 0019
/// What the user is told. No variant carries text, so no content can leak.
pub enum Notice {
    Started,
    Stopped,
    ClipboardCleared,           // replacing failed; the selection was cleared
    OriginalStillOnClipboard,   // replacing and clearing both failed
    PrimaryNotUpdated,          // 0024: the mirror write failed
    BackendLost,                // the display connection ended
}
pub trait Notifier { fn notify(&mut self, notice: Notice); }

// crates/sagiri/src/session/core.rs                   // 0019
pub enum Command { Start, Stop, Toggle, Status, Quit }
pub enum Event {
    /// The selection has a new owner.
    Changed(Selection),
    /// A request from the control socket. The reply is the state after the command.
    Command { command: Command, reply: std::sync::mpsc::Sender<bool> },
    /// The hotkey was pressed.
    Toggle,
    BackendLost,
    /// SIGINT or SIGTERM.
    Shutdown,
}
pub enum Outcome { Inactive, Replaced, Own, Skipped, Failed }
pub enum Exit { Quit, Shutdown, BackendLost }
pub struct Session { /* .. */ }
impl Session {
    pub fn new(engine: Engine, clipboard: Box<dyn SelectionClipboard>,
               notifier: Box<dyn Notifier>) -> Self;
    pub fn with_primary(self, mode: PrimaryMode) -> Self;          // 0024
    pub fn is_active(&self) -> bool;
    pub fn set_active(&mut self, active: bool);
    pub fn on_change(&mut self, selection: Selection) -> Outcome;
}
pub fn run_loop(session: &mut Session, events: &std::sync::mpsc::Receiver<Event>) -> Exit;
```

Threads in `sagiri-session`: the main thread owns the `Session` and with it the clipboard.
Every other thread (control socket, signals, X11 watcher) only sends `Event`s. Nothing but
the main thread reads or writes a selection.

Control protocol on the socket (ticket 0020): the client sends one line, `start`, `stop`,
`toggle`, `status` or `quit`. The server answers with one line, `active`, `inactive` or
`error <reason>`, and closes the connection. A reason is a fixed text.

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
| Session socket | `$XDG_RUNTIME_DIR/sagiri/session.sock` | none: an error |

- An unset, empty or relative `XDG_*` value is ignored and the default is used.
- `XDG_RUNTIME_DIR` is the exception: it has no default. If it is unset, empty or relative,
  `sagiri-session` needs `--socket <PATH>`. The directory `sagiri/` is created with mode
  0700. The socket carries commands and states, never text from the clipboard.
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
| X11 events and key grab | `x11rb` 0.13 | feature `xfixes`; Linux-only dependency; the same version `arboard` already brings in |
| Signals | `signal-hook` 0.3 | the same version `crossterm` already brings in; do not add 0.4 next to it |

Rules for the two crates added from ticket 0020 on:

- Both are already in `Cargo.lock` through other crates. After adding them,
  `cargo tree -p sagiri -i <crate>` must show one version.
- `x11rb` goes under `[target.'cfg(target_os = "linux")'.dependencies]`, and the code that
  uses it under `#[cfg(target_os = "linux")]`. No cargo feature: `scripts/check.sh` must
  build and test the same code on every machine.
- `x11rb` forbids unsafe code unless its `allow-unsafe-code` feature is on. Do not enable
  that feature.

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
- Release packaging beyond the local PKGBUILD of ticket 0029: tagged releases, an AUR
  package, man pages, shell completions
- Session mode on Wayland
- A systemd user unit for `sagiri-session`
- Starting the session process from `sagiri-session toggle` when none is running
- Following keyboard layout changes while `sagiri-session` runs
- Keeping rich clipboard formats in session mode when they hold no finding
