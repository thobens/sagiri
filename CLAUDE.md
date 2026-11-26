# Sagiri

Obfuscates pasted text with consistent, key-derived fake values. Rust workspace, Unix only.

## Commands

- Build: `cargo build`
- Run: `cargo run -p sagiri --bin sagiri -- <args>` (or `--bin sagiri-cli`)
- Release build and install: `make release`, `make install PREFIX=~/.local` (install does not build)
- Quality gate: `scripts/check.sh` (fmt, clippy with `-D warnings`, tests)
- Single test: `cargo test -p <crate> <test_name>`
- Vulnerability check: `scripts/audit.sh` (`cargo auditable` release build, `cargo audit bin`; needs network, not in `check.sh`)

## Layout

- `crates/sagiri-core`: library with detection and pseudonymization. No terminal or
  clipboard I/O, no printing.
- `crates/sagiri`: library plus the `sagiri` binary. CLI, filter mode, TUI, clipboard.

## Hard rules

- Never log, print or persist pasted text or key material. Error messages name files, keys
  and positions, never content.
- No `unsafe` (forbidden by workspace lint).
- No `rand` crate in `sagiri-core`. All randomness comes from `Pseudonymizer`.
- Golden vectors are a compatibility contract. Changing one is a breaking change.
- Test data is fictional: documentation IP ranges, published test card numbers.
- Every public item of `sagiri-core` has a doc comment, and a new public entry point comes
  with an example that runs as a doc test.

## Tickets

- Tickets live in `tickets/`. Start with `tickets/README.md`.
- Branch naming: `ticket/NNNN`. Default branch: `master`.
