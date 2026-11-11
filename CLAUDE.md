# Sagiri

Obfuscates pasted text with consistent, key-derived fake values. Rust workspace, Unix only.

## Commands

- Build: `cargo build`
- Run: `cargo run -p sagiri -- <args>`
- Quality gate: `scripts/check.sh` (fmt, clippy with `-D warnings`, tests)
- Single test: `cargo test -p <crate> <test_name>`

## Layout

- `crates/sagiri-core`: library with detection and pseudonymization. No terminal or
  clipboard I/O, no printing.
- `crates/sagiri`: the `sagiri` binary. CLI, filter mode, TUI, clipboard.

## Hard rules

- Never log, print or persist pasted text or key material. Error messages name files, keys
  and positions, never content.
- No `unsafe` (forbidden by workspace lint).
- No `rand` crate in `sagiri-core`. All randomness comes from `Pseudonymizer`.
- Golden vectors are a compatibility contract. Changing one is a breaking change.
- Test data is fictional: documentation IP ranges, published test card numbers.

## Tickets

- Tickets live in `tickets/`. Start with `tickets/README.md`.
- Branch naming: `ticket/NNNN`. Default branch: `master`.
