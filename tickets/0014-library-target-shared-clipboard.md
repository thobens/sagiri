---
id: "0014"
title: Library target and shared clipboard module
status: done
depends_on: []
---

# 0014 — Library target and shared clipboard module

## Why

Two more binaries are coming: `sagiri-cli` (ticket 0016) and `sagiri-session` (ticket 0020).
They need the engine set-up, the filter and the clipboard code that today live inside the
`sagiri` binary. This ticket turns that code into a library target of the same package and
moves the clipboard module out of the TUI. It changes no behaviour.

## Scope

In `crates/sagiri`:

- New `src/lib.rs` with `pub mod clipboard; pub mod filter; pub mod setup; pub mod tui;` and
  a short crate-level doc comment.
- `src/tui/clipboard.rs` moves to `src/clipboard.rs` with `git mv`. Its content stays the
  same: the `Clipboard` trait, `SystemClipboard`, and the test-only `FakeClipboard`.
- `src/tui/mod.rs` and `src/tui/app.rs` import from `crate::clipboard`.
- `src/main.rs` drops its `mod` lines and uses the library (`sagiri::filter`,
  `sagiri::setup`, `sagiri::tui`). Its logic is unchanged.
- Items that `main.rs` needs become `pub`. Nothing else changes visibility.
- `CLAUDE.md`: the layout line for `crates/sagiri` says that it is a library plus the
  `sagiri` binary.

## Out of scope

- Any change in behaviour, output, flags or exit codes.
- New dependencies, new binaries, new modules other than `lib.rs`.
- The selection-aware clipboard (ticket 0015).
- Edits to files under `crates/sagiri/tests` or to `crates/sagiri-core`.

## Design notes

- **One package, several binaries.** Cargo builds `src/lib.rs` as the library `sagiri` and
  `src/main.rs` as the binary `sagiri`. Later tickets add `src/bin/*.rs`. A separate crate
  for the shared code is not needed.
- **The integration tests are the proof.** They start the `sagiri` binary and must pass
  without being touched.
- **Unit tests move with their code.** The tests in `tui/app.rs` keep using
  `FakeClipboard`, now from `crate::clipboard`.
- The library and the binary share a name. That is the normal Cargo layout and needs no
  `[lib]` or `[[bin]]` table.

## Acceptance criteria

- [ ] `crates/sagiri/src/lib.rs` exists; `crates/sagiri/src/tui/clipboard.rs` does not.
- [ ] `grep -rn "tui::clipboard" crates/` prints nothing.
- [ ] `cargo metadata --no-deps --format-version 1` lists, for the package `sagiri`, one
      target of kind `lib` and one of kind `bin`, both named `sagiri`.
- [ ] `git diff master -- crates/sagiri/tests crates/sagiri-core crates/sagiri/Cargo.toml`
      is empty.
- [ ] The number of tests run by `cargo test -p sagiri` is not lower than on `master`. The
      report gives both numbers.
- [ ] `scripts/check.sh` passes.

## Test plan

- No new tests. The existing unit and integration tests cover the refactoring.
- Run `cargo test -p sagiri` on `master` and on the branch and compare the totals.
