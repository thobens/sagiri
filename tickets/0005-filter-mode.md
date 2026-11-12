---
id: "0005"
title: "Filter mode: stdin to stdout"
status: done
depends_on: ["0004"]
---

# 0005 — Filter mode: stdin to stdout

## Why

Until now the library can only be exercised from unit tests. Filter mode is the smallest
complete product: text goes in, obfuscated text comes out, and a second run gives the same
replacements. It makes end-to-end tests possible and gives the TUI (ticket 0010) a tested
setup path to reuse.

## Scope

All in `crates/sagiri`.

- **`setup.rs`**
  - `pub struct Options { pub key_file: Option<PathBuf> }`
  - `pub fn build_engine(opts: &Options) -> anyhow::Result<Engine>`: resolve the key path
    with `sagiri_core::key::resolve_path(opts.key_file, &std::env::var_os)`, call
    `Key::load_or_create`, return
    `Engine::new(Pseudonymizer::new(&key), default_rules())`.
- **`main.rs`**
  - Global option `--key-file <PATH>`.
  - Mode decision in a pure function `fn mode(stdin_is_terminal: bool) -> Mode`, using
    `std::io::IsTerminal`:
    - stdin is not a terminal: filter mode;
    - stdin is a terminal: print
      `sagiri: interactive mode is not available yet; pipe text into sagiri` to stderr and
      exit 2. Ticket 0010 replaces this branch.
- **`filter.rs`**
  - `pub fn run(engine: &Engine, input: impl Read, output: impl Write) -> anyhow::Result<()>`
  - Read all of the input, require valid UTF-8, write `engine.obfuscate(text)`, flush.
  - The output is exactly the transformed text. No newline is added or removed. Empty
    input gives empty output.
- **Errors and exit codes**
  - 0: success. 1: runtime error. 2: usage error (clap's default, and the terminal case
    above).
  - Errors go to stderr as `sagiri: <message>`. A message names paths and causes, never
    input text.
  - Invalid UTF-8: `sagiri: input is not valid UTF-8`, exit 1, nothing on stdout.
  - A broken pipe on stdout (for example `sagiri < big.log | head`) ends the program
    quietly with exit 0.
- **`README.md`**: a "Usage" section showing `sagiri < input.txt > output.txt`, where the
  key is stored, and that deleting the key changes all replacements.

## Out of scope

- The TUI (ticket 0010).
- File arguments (`sagiri file.log`), in-place editing, streaming line by line.
- The config file and `--config` (ticket 0012).
- Any flag to list or explain findings.

## Design notes

- `build_engine` is the single place that turns options into an `Engine`. Tickets 0010 and
  0012 extend it rather than building their own.
- The whole input is read into memory. Pasted text is small; streaming would complicate
  rules that look at neighbouring characters.
- The mode decision is a pure function because a test process never has a terminal on
  stdin, so the terminal branch cannot be reached through `assert_cmd`.
- **Tests must never touch the real key.** Every test that starts the binary passes
  `--key-file` inside a temporary directory, or sets `SAGIRI_KEY_FILE` / `XDG_DATA_HOME` /
  `HOME` to temporary directories on the child process (`Command::env`).

## Acceptance criteria

End-to-end tests in `crates/sagiri/tests/filter.rs` with `assert_cmd` and `tempfile`:

- [ ] `reference_example_is_consistent_across_runs`: two separate runs with the same key
      file, inputs `server 172.55.0.1 is down` and `ping 172.55.0.1 failed`. Both outputs
      contain the same replacement address, it is not `172.55.0.1`, and the surrounding
      words are unchanged.
- [ ] `different_keys_give_different_replacements`.
- [ ] `text_without_findings_is_unchanged`: output bytes equal input bytes, including a
      missing trailing newline and CRLF line endings.
- [ ] `empty_input_gives_empty_output`.
- [ ] `key_is_created_under_xdg_data_home`: with only `XDG_DATA_HOME` and `HOME` set on the
      child, the first run creates `$XDG_DATA_HOME/sagiri/key` with mode 0600.
- [ ] `env_key_file_is_used` and `flag_wins_over_env`.
- [ ] `invalid_utf8_is_rejected`: exit code 1, empty stdout, stderr mentions UTF-8 and
      does not contain the input bytes.
- [ ] `insecure_key_file_is_rejected`: a key file with mode 0644 gives exit code 1 and a
      message naming the path.
- [ ] `large_input`: 5 MiB of log-like lines containing addresses is processed
      successfully and the line count is unchanged.

Unit tests:

- [ ] `mode(false)` is filter mode; `mode(true)` is the terminal branch.
- [ ] `filter::run` with a writer that returns `BrokenPipe` returns `Ok`.

And:

- [ ] With `K="$(mktemp -d)/key"`, the command
      `printf 'a 172.55.0.1 b\n' | cargo run -q -p sagiri -- --key-file "$K"` prints the
      line with a different address; running it again prints the identical line.
- [ ] `scripts/check.sh` passes.

## Test plan

- The tests named above.
- Run the manual command from the acceptance criteria twice and include both output lines
  in the report.
