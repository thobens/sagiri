---
id: "0037"
title: Documented `sagiri-core` API with testable examples
status: todo
depends_on: []
---

# 0037 — Documented `sagiri-core` API with testable examples

## Why

`sagiri-core` is the part of Sagiri that other code builds on: the three binaries today,
other tools later. Its public items have short comments or none, and not one example.
Somebody who wants to obfuscate a string from their own program has to read the binaries
to learn how an engine is put together. Examples that are compiled and run with the tests
show how the API is meant to be used and cannot go stale without failing the quality gate.

## Scope

### Doc comments

- Every public item of `sagiri-core` has a doc comment: the crate, every module, struct,
  enum, trait, type alias, constant, function and method, and every public field and enum
  variant.
- `crates/sagiri-core/src/lib.rs` gets `#![deny(missing_docs)]` next to the existing
  `deny` line, so that the next undocumented public item fails `scripts/check.sh`.
- A comment says what the item is for and what a caller can rely on, not how it is
  implemented. The guarantees that matter to a caller are stated where they apply:
  - the same key and input always give the same output, in every future version;
  - which form of a value is canonical, so that two spellings map to one replacement;
  - that a replacement never contains a line break.
- A function that returns `Result` has an `# Errors` section that names the cases. A
  function that can panic has a `# Panics` section.
- Links between items use intra-doc links.
- Existing comments are kept where they are correct and extended where they are thin.

### Examples

The crate documentation in `lib.rs` opens with an overview of the modules and a quick
start: test key, `Pseudonymizer`, `Engine` with `default_rules()`, one `obfuscate` call.

Each of the following has at least one example that runs as a doc test:

| Module | Items |
|---|---|
| `key` | `Key::from_bytes`, `Key::load_or_create`, `resolve_path` |
| `pseudo` | `Pseudonymizer::new`, `stream`, `permute`, `permute_within`, `mimic`; `DetStream::below`, `pick` |
| `engine` | `Engine::new`, `with_allowlist`, `analyze`, `obfuscate`; `render` |
| `rule` | `Kind::ALL`, `id`, `label`; the `Rule` trait |
| `rules` | `default_rules`, `TermRule::new`, `ip::pseudonymize_v4`, `ip::pseudonymize_v6`, `hostname::has_known_suffix`, `hostname::pseudonymize` |
| `config` | `Config::load`, `resolve_path`, `Allowlist::new` and `matches`, `RuleToggles::enabled` |
| `paths` | `config_dir_with`, `data_dir_with`, `runtime_dir_with` |

- The example for the `Rule` trait implements a small rule of its own, uses
  `Context::pseudo`, `Context::nested` and `Context::is_allowed` in it, and runs it in an
  `Engine`.
- The example for `render` keeps one finding and replaces another, and reads the
  `replaced` flag of the spans.

### Rules for examples

- **No real files and no real environment.** The key comes from `Key::from_bytes` with the
  test key (`0x00, 0x01, … 0x1f`). File examples use a temporary directory (`tempfile` is
  a development dependency and available to doc tests). Path examples use the `*_with`
  functions with a closure instead of the process environment. No example reads or writes
  the user's key file, config file or XDG directories.
- **Fictional data only**: documentation address ranges (`192.0.2.0/24`,
  `198.51.100.0/24`, `203.0.113.0/24`, `2001:db8::/32`), names under `example.com`,
  `example.org` and `example.net`, published test card numbers.
- **Assertions, not output.** An example shows its result with `assert!` and
  `assert_eq!`. It prints nothing.
- **Literal replacements only from golden vectors.** An example may assert a literal
  replacement value only if exactly that input and output are already a golden vector in
  `crates/sagiri-core/tests/`. Otherwise it asserts properties: the output differs from
  the input, has the same shape, and is the same on a second call.
- **Every example runs.** No `ignore`, no `no_run`. `compile_fail` is allowed where the
  point of the example is that something does not compile. Set-up lines that distract are
  hidden with `#`.
- An example is short enough to be read at a glance. A helper that several examples need
  is repeated as hidden lines, not added to the public API.

### Quality gate

- `scripts/check.sh` gets one more command, after the tests:
  `RUSTDOCFLAGS='-D warnings' cargo doc -p sagiri-core --no-deps --locked`. A broken
  intra-doc link then fails the gate.
- Doc tests need no new command: `cargo test --workspace` already runs them.

### Documentation

- `CLAUDE.md`, "Hard rules": one line saying that every public item of `sagiri-core` is
  documented and that a new public entry point comes with a doc test.
- `README.md`, "Checks": the gate also builds the documentation of `sagiri-core`;
  `cargo doc -p sagiri-core --no-deps --open` shows it.

## Out of scope

- Any change of behaviour, signature or visibility. If an item looks public by accident,
  it is documented as it is and named in the report.
- The library target of `crates/sagiri`. It is glue for the three binaries and not an API
  for others (backlog).
- New golden vectors, and any change to an existing one.
- Publishing the documentation (docs.rs, GitHub Pages) and publishing the crate.
- Rewriting the user-facing `README.md` beyond the lines named above.
- New unit or integration tests. Doc tests show usage; the existing tests stay the proof
  of correctness.

## Design notes

- **`deny(missing_docs)` in the crate, not in the workspace lints.** The rule is for the
  library only. The binaries' crate keeps `[lints] workspace = true` and is not affected.
- **Why literal values are restricted.** A literal replacement in an example is one more
  place that pins the output of a key. The golden vector files are the compatibility
  contract; examples must not grow into a second one that nobody maintains on purpose.
  Reusing an existing vector adds no new promise.
- **Why examples avoid the default paths.** Doc tests run on developer machines and in
  CI. An example that calls `Key::load_or_create` on the default path would create a real
  key file in the home directory of whoever runs the tests.
- **Printing.** The crate denies `print_stdout` and `print_stderr`. Doc tests are compiled
  as separate crates, so the lint does not reach them, but an example that prints would
  teach the wrong habit for code that handles sensitive text.
- **The pinned API in `tickets/README.md` does not change.** If writing a comment shows
  that an item behaves differently from what the README pins, stop and report.
- **`cargo doc` for one package.** The workspace-wide command would also document the
  binaries' crate, which is out of scope here.

## Acceptance criteria

- [ ] `cargo test -p sagiri-core --doc --locked` passes. The report gives the number of
      doc tests and a table that maps every item of the scope table to its example.
- [ ] `RUSTDOCFLAGS='-D warnings' cargo doc -p sagiri-core --no-deps --locked` passes.
- [ ] `scripts/check.sh` passes and contains the `cargo doc` command.
- [ ] With the doc comment of one public function removed, `scripts/check.sh` fails with
      the `missing_docs` error. With one intra-doc link broken, it fails in `cargo doc`.
      Both changes are reverted; the report shows the two failures.
- [ ] No doc test is marked `ignore` or `no_run`: a `grep` for the two words on the code
      fence lines of the doc comments in `crates/sagiri-core/src` finds nothing. The
      report shows the command.
- [ ] `grep -rnE 'println!|eprintln!|dbg!' crates/sagiri-core/src` finds nothing new
      compared to `master`.
- [ ] `git diff master -- crates/sagiri-core/tests` is empty.
- [ ] `git diff master -- crates/sagiri-core/src` changes only comment lines and the one
      attribute line in `lib.rs`. The reviewer checks this on the diff.
- [ ] No example names a path outside a temporary directory, and none reads the process
      environment. The reviewer checks this by reading the examples.
- [ ] Every literal replacement value in an example also occurs in a golden vector file
      (the report lists the pairs and their test files, or states that there are none).
- [ ] `CLAUDE.md` and `README.md` have the lines of the scope.

## Test plan

- The commands of the acceptance criteria.
- Run the doc tests with `HOME` and the `XDG_*` variables pointing to an empty temporary
  directory, and check afterwards that the directory is still empty. This shows that no
  example touches the default paths.
