# Sagiri — MVP tickets in `tickets/`

## Context

Sagiri obfuscates pasted text: it finds sensitive values (IPs, URLs, PII, PCI data) and
replaces them with realistic-looking fake values, **consistently** — the same original always
yields the same replacement, across pastes and across sessions.

The repo is empty (only an empty `README.md`; `master` also has an empty `CLAUDE.md` and the
`implementer` / `reviewer` agents). This task writes the ticket files that describe the path
to a usable MVP, starting with project scaffolding. **No Rust code is written in this task.**

Product decisions:

- **Interface:** terminal UI (paste in, see result, copy out).
- **One-way only:** replacements are derived from a secret key; no original values are ever
  stored; no restore feature.
- **Horizon:** detailed tickets through a usable MVP; later ideas as a short backlog list.

## Step 0 — put this branch on top of `master`

This worktree branch starts at `dc7a78f`, but `master` was since amended to `257eefd`
(adds `.claude/agents/*` and `CLAUDE.md`). The two are unrelated root commits, so the branch
could not be merged cleanly later. The worktree is clean and holds nothing `master` lacks, so:
`git reset --hard master` in this worktree, then write the tickets.

## Files to create

`tickets/README.md` plus 13 ticket files. Nothing else is touched.

### Ticket format (fits the existing agents)

File `tickets/NNNN-slug.md`, id `NNNN` → implementer branch `ticket/NNNN`.

```markdown
---
id: "0004"
title: IP address rule (IPv4 + IPv6)
status: todo            # todo | in-progress | done
depends_on: ["0002", "0003"]
---
## Why                  (2-3 sentences)
## Scope                (deliverables)
## Out of scope
## Design notes         (pinned decisions, API the ticket must expose/use)
## Acceptance criteria  (checkboxes, each verifiable by a command or a named test)
## Test plan
```

Tickets are self-contained (the implementer agent only sees its ticket) but all point to the
shared section of `tickets/README.md`.

### `tickets/README.md`

- Workflow: how implementer/reviewer use tickets, status values, id rules.
- Definition of done for every ticket: `scripts/check.sh` green, tests added, pasted text
  never logged or written to disk, golden vectors unchanged unless the ticket says so.
- Architecture at a glance (below), index table, dependency graph, backlog.

### The tickets

| ID | Title | Depends on | Core content |
|---|---|---|---|
| 0001 | Project scaffolding | – | Cargo workspace (edition 2024): `crates/sagiri-core` (lib) + `crates/sagiri` (bin, clap, `--version`). Workspace lints (`unsafe_code` forbidden; `print_stdout`/`print_stderr`/`dbg_macro` denied in core), `rust-toolchain.toml`, `.gitignore`, `scripts/check.sh` (fmt, clippy `-D warnings`, test), README, CLAUDE.md with commands and hard rules. No LICENSE yet, `publish = false`. |
| 0002 | Keyed pseudonymization primitives + key file | 0001 | `paths` module implementing the XDG Base Directory rules once for the whole project (`config_dir()`, `data_dir()`; see below). `Key` (32 bytes, zeroized, redacted Debug), `load_or_create` (0600; lookup order: `--key-file` > `SAGIRI_KEY_FILE` > `$XDG_DATA_HOME/sagiri/key`). `Pseudonymizer`: `stream(domain, input)` = HMAC-SHA256 counter-mode byte stream with unbiased `below(n)`/`pick`; `permute(domain, bits, v)` = keyed Feistel bijection, plus cycle-walking over a subset. Golden vectors with a fixed test key. |
| 0003 | Detection & replacement engine | 0002 | `Kind` enum (all MVP kinds up front), `Rule` trait (`detect` → byte ranges, `replace(original, &Context)`), `Engine::analyze` → `Vec<Finding>`, `Engine::render(text, findings, filter)` → output text + output spans. Deterministic overlap resolution (earliest start, then longest, then rule order). Findings never span a newline. |
| 0004 | IP address rule (IPv4 + IPv6) | 0003 | Validated detection; category preserved (10/8, 172.16/12, 192.168/16, link-local, CGNAT, public); collision-free within a category via `permute`; loopback, unspecified, broadcast, netmasks, multicast left alone. |
| 0005 | Filter mode: stdin → stdout | 0004 | If stdin is not a terminal, obfuscate stdin to stdout. `--key-file`. Shared `build_engine()`. `assert_cmd` end-to-end tests, including the reference example: two separate runs map `172.55.0.1` to the same value. |
| 0006 | Hostname & URL rules | 0003 | Label-wise hostname mapping (public suffix kept, parents shared: `git.acme.com` and `www.acme.com` share the `acme.com` pseudonym). URL: scheme/port/path kept; host, userinfo and query values replaced; host mapping identical to bare hostnames and to 0004 for IP hosts. False-positive table (`main.rs`, `foo.bar()`, …). |
| 0007 | Email address rule | 0006 | Local part pseudonymized (separators and length kept), domain through the hostname mapping, so emails, URLs and bare hosts stay consistent. |
| 0008 | Payment card & IBAN rules | 0003 | Cards: 13–19 digits, Luhn + brand prefix check; replacement keeps brand prefix, length, grouping and is Luhn-valid. IBAN: mod-97 + country length; replacement keeps country, grouping, character classes and has valid check digits. |
| 0009 | Phone number rule | 0003 | International `+CC…` form only, country code validated; replacement keeps `+CC`, digit count and formatting. |
| 0010 | TUI: paste → obfuscate → copy | 0005 | ratatui + crossterm + arboard. Original / Obfuscated panes, synchronized scroll, status line. Bracketed paste (CR → LF normalization) and Ctrl+V; `y`/Enter copies the result. Pure `App` state + `Clipboard` trait so it is testable with `TestBackend`; terminal restored on panic. |
| 0011 | TUI: findings review | 0010 | Per-kind highlighting in both panes; findings list (`IPv4 172.55.0.1 → 126.143.4.12 ×3`); toggle a single finding or a whole kind off; decisions remembered for the session only (never on disk). |
| 0012 | Config file | 0005 | TOML at the XDG config path: `$XDG_CONFIG_HOME/sagiri/config.toml`, falling back to `~/.config/sagiri/config.toml` (lookup order: `--config` > `SAGIRI_CONFIG` > XDG path; a missing file means defaults). Enable/disable rules, allowlist (exact, `*.domain`, CIDR), custom literal terms (`Kind::Term`) replaced by same-shape pseudo-words. Unknown keys are errors. Used by filter mode and TUI. |
| 0013 | CI workflow | 0001 | GitHub Actions running `scripts/check.sh` on push and PR. |

After 0003, tickets 0004, 0006→0007, 0008 and 0009 are independent and can be implemented in
parallel (each adds one line to `default_rules()`).

### Architecture pinned in `tickets/README.md`

```
crates/sagiri-core/src/   paths.rs  key.rs  pseudo.rs  rule.rs  engine.rs  config.rs  rules/{mod,ip,hostname,url,email,card,iban,phone,term}.rs
crates/sagiri/src/        main.rs  filter.rs  tui/{app,ui,clipboard}.rs
```

- **Consistency = derivation, not storage.** Every replacement is a pure function of
  (key, kind, canonical form of the original). Canonical form makes `ACME.com`/`acme.com`
  or `4111 1111…`/`4111-1111…` map alike; the output re-applies the input's formatting.
- **Stability contract.** No `rand` crate in the derivation path (its distributions are not
  stable across versions); own HMAC-based stream, domain-separated with a `sagiri/v1` label.
  Golden-vector tests per rule; changing one is a breaking change.
- **Composite values delegate.** URL and email rules call the hostname/IP mapping functions,
  which is what keeps a host consistent wherever it appears.
- **Two-phase engine** (`analyze` then `render` with a filter) so the TUI can toggle findings
  without re-detecting.
- **XDG Base Directory spec for every file Sagiri touches**, resolved in one place
  (`sagiri-core/src/paths.rs`, added in 0002, reused by 0012):
  - config: `$XDG_CONFIG_HOME/sagiri/config.toml`, default `~/.config/sagiri/config.toml`
  - key: `$XDG_DATA_HOME/sagiri/key`, default `~/.local/share/sagiri/key`
  - per the spec, an unset, empty or relative `XDG_*` value is ignored and the default used
  - same layout on Linux and macOS (no `~/Library/...`); explicit flag or `SAGIRI_*` env
    variable always wins over the XDG path
  - tests set `XDG_CONFIG_HOME` / `XDG_DATA_HOME` / `HOME` to temp dirs and assert the
    resolved paths, including the empty and relative cases

### Backlog section (one line each, no tickets)

MAC addresses · secrets/API keys/JWTs · national-format phone numbers · national IDs (AHV,
SSN, …) · person names (dictionary or NER spike) · prefix-preserving IP mode and CIDR-aware
mapping · optional URL path obfuscation · profiles / multiple keys, key rotation command ·
auto-copy on paste · OSC 52 clipboard over SSH · file arguments · UUIDs, cloud account IDs ·
release packaging.

## Default decisions (open to change before the tickets are written)

- **0005 filter mode and 0013 CI go beyond the original request.** Filter mode is small and is
  what makes end-to-end tests possible; CI assumes GitHub (no remote is configured yet).
- Key lives in the XDG data dir (`$XDG_DATA_HOME/sagiri/key`), not next to the config in
  `$XDG_CONFIG_HOME`, so it is not swept into a dotfiles repo along with `config.toml`.
- IPs keep their category (private stays private). The reference example `172.55.0.1` is
  public, so it still maps to a random public address.
- URL paths are kept as-is; only host, credentials and query values are replaced.
- Hostnames keep their public suffix (`.com`, `.co.uk`).
- Phone numbers: only the international `+…` form in the MVP.

## Notes outside the tickets

- `reviewer.md` says "diff against main", but the branch is `master`.
- The implementer agent runs in its own worktree, so tickets are only visible to it once
  they are committed and on `master`. Committing is not part of this plan.

## Verification

- A shell check over `tickets/*.md`: ids unique and matching file names, every `depends_on`
  id exists, no dependency cycles, all six sections present in each ticket, index table in
  `tickets/README.md` lists exactly the files on disk.
- Read each ticket once more as the implementer would: is everything needed in the ticket or
  the README, and is every acceptance criterion checkable by a command or a named test?
- No cargo commands apply — this task creates no code.
