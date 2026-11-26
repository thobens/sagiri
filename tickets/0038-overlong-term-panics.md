---
id: "0038"
title: Reject a term that is too long instead of panicking
status: done
depends_on: ["0012", "0037"]
---

# 0038 — Reject a term that is too long instead of panicking

## Why

A very long entry in `terms` makes Sagiri panic at start-up.

`TermRule::new` turns every term into a case-insensitive regex and unwraps the result with
`.expect("an escaped literal is a valid regex")`. The escaped literal is always valid
syntax, but the `regex` crate also has a size limit for the compiled program, and a long
enough term exceeds it. `validate_terms`, which both `Config::load` and `TermRule::new`
call, only rejects a line break and a term shorter than two characters. So a config file
with one such term loads without error, and building the engine in `setup.rs` then panics
instead of returning a `ConfigError`.

Found while documenting `TermRule::new` for ticket 0037. Measured on that branch (`regex`
1.13.1) with a throwaway test that searched for the shortest single term whose regex fails
to build:

| Term made of | First length that panics |
|---|---|
| `a` | 131,070 characters |
| `ä` | 62,415 characters |
| `1` or `-` | 327,674 characters |

The numbers depend on the characters and on the `regex` version, so they can move with a
dependency update.

Ticket 0012 made every unusable config entry an error that names the key and hides the
value. A term that cannot be compiled has to fail the same way. A panic prints a Rust
panic report instead: it names neither the config file nor the entry.

## Scope

### `sagiri-core/src/rules/term.rs`

- `validate_terms` rejects a term that has more than `MAX_TERM_CHARS` characters after
  trimming, with `ConfigError::Invalid`, key `terms[<index>]`, reason
  `term is longer than 1000 characters`.
  - `MAX_TERM_CHARS` is a private constant with the value `1000`. Characters are counted
    with `chars().count()`, as the existing minimum is.
  - The check comes after the two existing checks, so their messages do not change.
- `TermRule::new` no longer unwraps the regex. A build error becomes
  `ConfigError::Invalid`, key `terms[<index>]`, reason `term cannot be compiled`. With the
  length limit in place this branch is not expected to be reachable; it exists so that no
  input and no `regex` update can turn a term into a panic.
- Both reasons are fixed texts. Neither message contains the term or any part of it.
- Doc comment of `TermRule::new`: the `# Panics` section that ticket 0037 added is removed,
  and `# Errors` names the two new cases. Its example stays a passing doc test.

### `README.md`

- The line in the config section that says terms need at least two characters also gives
  the maximum of 1000 characters.

Nothing else changes: the public API pinned in `tickets/README.md`, `Config`, the matching
of terms, and every existing message.

## Out of scope

- A limit on the number of terms, or on the total size of the config file.
- A limit for entries in `allow`. They are not compiled into a regex.
- Changing how terms are matched (one regex per term, case-insensitive, whole words).
- Raising the size limit of the `regex` crate with `RegexBuilder::size_limit`.

## Design notes

- **Why a fixed character limit.** The length at which `regex` gives up depends on the
  characters and on the crate version. A limit derived from it would change with a
  dependency update. 1000 characters is far above any realistic name or phrase and more
  than sixty times below the smallest measured failure.
- **Why the error mapping as well.** The limit alone would leave an `expect` whose
  correctness depends on numbers measured for three kinds of characters on one `regex`
  version. Characters with more case variants may fail earlier. Mapping the build error
  keeps the guarantee independent of those numbers.
- **The index, not the content.** As for every other `terms` error, the message names the
  position of the entry in the array and nothing from the entry.
- **Golden vectors are not affected.** No derivation changes; a term of up to 1000
  characters is replaced exactly as before.
- If the measurements above cannot be reproduced, or a term of at most 1000 characters is
  found whose regex fails to build, the report says so with the input class (not a real
  term).

## Acceptance criteria

Unit tests in `rules/term.rs`, next to `terms_are_validated`:

- [ ] `overlong_term_is_error`: a term of 1001 `a` gives `ConfigError::Invalid` whose
      message contains `terms[0]` and `term is longer than 1000 characters`, and does not
      contain `aaaa`. With a valid term in front of it the message contains `terms[1]`.
- [ ] `term_at_the_limit_is_accepted`: a term of exactly 1000 characters builds a
      `TermRule`, for `a`, for `ä` and for `1`, and the rule finds the term in a text that
      contains it.
- [ ] `overlong_term_is_measured_after_trimming`: a term of 1000 `a` surrounded by spaces
      is accepted.
- [ ] `huge_term_does_not_panic`: `TermRule::new` with one term of 200,000 `a` returns an
      error and does not panic.

Unit test in `config.rs`:

- [ ] `overlong_term_is_rejected_on_load`: a config file whose only term has 1001
      characters gives an error whose message contains `config.toml`, `terms[0]` and the
      reason.

End-to-end test in `crates/sagiri/tests/config.rs`:

- [ ] `overlong_term_exits_1`: with a config file holding one term of 200,000 `a`,
      `sagiri` exits 1, stdout is empty, stderr names `terms[0]`, and stderr contains
      neither `panicked` nor `aaaa`.

And:

- [ ] `grep -n 'expect(' crates/sagiri-core/src/rules/term.rs` finds nothing outside the
      test module.
- [ ] `cargo test -p sagiri-core --doc --locked` passes, and the doc comment of
      `TermRule::new` has no `# Panics` section.
- [ ] Every existing test passes unchanged, including `terms_are_validated`,
      `invalid_terms_are_rejected_with_index` and all golden vectors.
- [ ] `scripts/check.sh` passes.

## Test plan

- Table-driven unit tests with terms built by `"a".repeat(n)`; no long literal in the
  source.
- One end-to-end test with temporary key and config files, next to the existing tests for
  invalid config entries.
