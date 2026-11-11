---
id: "0001"
title: Project scaffolding
status: todo
depends_on: []
---

# 0001 — Project scaffolding

## Why

The repository holds no code. Every later ticket needs a workspace that builds, one command
that acts as the quality gate, and the project conventions written down where an agent or a
new contributor will find them.

## Scope

- **Workspace.** Root `Cargo.toml` as a virtual workspace:
  - `members = ["crates/*"]`, `resolver = "3"`
  - `[workspace.package]`: `version = "0.1.0"`, `edition = "2024"`,
    `rust-version = "1.88"`, `publish = false`
  - `[workspace.lints.rust]`: `unsafe_code = "forbid"`
  - `[workspace.lints.clippy]`: `all = { level = "warn", priority = -1 }`,
    `dbg_macro = "deny"`
- **`crates/sagiri-core`** (library). Inherits package fields and lints from the workspace.
  `src/lib.rs` contains a crate doc comment and
  `#![deny(clippy::print_stdout, clippy::print_stderr)]`. No other code yet.
- **`crates/sagiri`** (binary named `sagiri`). Depends on `sagiri-core` by path and on
  `clap` (derive). `main.rs` parses arguments with clap and supports `--version` and
  `--help`. With no arguments it prints the help text and exits 0. The `about` text is one
  sentence describing Sagiri.
- **`rust-toolchain.toml`**: `channel = "stable"`, components `rustfmt` and `clippy`.
- **`.gitignore`**: `/target` and `.claude/worktrees/`.
- **`scripts/check.sh`** (executable, `set -euo pipefail`), run from the repository root:
  1. `cargo fmt --all --check`
  2. `cargo clippy --workspace --all-targets --locked -- -D warnings`
  3. `cargo test --workspace --locked`
- **`Cargo.lock`** is committed.
- **`README.md`**: what Sagiri is (two or three sentences), status "pre-MVP", how to build
  and run, how to run the checks, pointers to `PLAN.md` and `tickets/README.md`.
- **`CLAUDE.md`** (currently empty), at most 40 lines:
  - commands: build, run, `scripts/check.sh`, running a single test
  - layout: the two crates and what belongs in each
  - hard rules: never log, print or persist pasted text or key material; no `unsafe`; no
    `rand` crate in `sagiri-core`; golden vectors are a compatibility contract
  - tickets: where they live, branch naming `ticket/NNNN`, default branch `master`
- **Smoke test** `crates/sagiri/tests/cli.rs` using `assert_cmd`: `--version` succeeds and
  prints `sagiri 0.1.0`.

## Out of scope

- Any detection, pseudonymization or TUI code.
- A LICENSE file and the `license` field. The licence is not decided.
- CI configuration (ticket 0013).
- `rustfmt.toml`, `clippy.toml`, `deny.toml`: defaults are fine for now.

## Design notes

- Two crates, not one, so the library never pulls in terminal or clipboard dependencies
  and can be reused by other front-ends later.
- The print lints are denied in the library only. The library must never write pasted text
  anywhere; the binary legitimately prints.
- `rust-version` 1.88 is the minimum required by the planned dependencies (ratatui 0.30,
  sha2 0.11). Raise it if a dependency needs more. There is no MSRV test.
- `resolver` must be set explicitly because a virtual workspace has no root package edition
  to infer it from.

## Acceptance criteria

- [ ] `scripts/check.sh` exits 0 on a clean checkout.
- [ ] `cargo run -q -p sagiri -- --version` prints `sagiri 0.1.0`.
- [ ] `cargo run -q -p sagiri -- --help` prints a usage text that contains the `about`
      sentence.
- [ ] Adding `println!("x");` to `crates/sagiri-core/src/lib.rs` makes `scripts/check.sh`
      fail (verify, then remove it).
- [ ] Adding an `unsafe {}` block to either crate makes `scripts/check.sh` fail (verify,
      then remove it).
- [ ] `git status` is clean after a build: `target/` is ignored, `Cargo.lock` is tracked.
- [ ] `CLAUDE.md` and `README.md` contain the items listed under Scope.

## Test plan

- `crates/sagiri/tests/cli.rs`: `version_flag_prints_version`.
- Run `scripts/check.sh`.
- Do the two negative checks from the acceptance criteria by hand and state the result in
  the report.
