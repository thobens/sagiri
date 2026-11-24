---
id: "0016"
title: "sagiri-cli: selectable input"
status: done
depends_on: ["0015"]
---

# 0016 — `sagiri-cli`: selectable input

## Why

`sagiri` decides between the TUI and filter mode by looking at stdin, which suits a person
at a terminal. Scripts and key bindings need a tool that never opens a UI and says where
its input comes from. This ticket adds that tool, `sagiri-cli`, with a selectable input.
Selectable output follows in ticket 0017.

## Scope

In `crates/sagiri`.

### Binary

- `src/bin/sagiri-cli.rs`: argument parsing, wiring, exit codes. No logic beyond that.
- `Cargo.toml`: `default-run = "sagiri"`, so that `cargo run -p sagiri` keeps working now
  that the package has two binaries.
- Command line:

  ```
  sagiri-cli [-i, --in <stdin|clipboard|primary>] [-n, --trim-newline]
             [--key-file <PATH>] [--config <PATH>]
  ```

  `--version` and `--help` as in `sagiri`. `--key-file` and `--config` mean the same as
  there and go through `setup::build_engine`.

### Logic (`src/cli.rs`)

- `pub enum Source { Stdin, Clipboard, Primary }`.
- `pub fn resolve_input(flag: Option<Source>, stdin_is_terminal: bool)
  -> Result<Source, NoInput>`:
  - a flag wins;
  - no flag and stdin is not a terminal: `Stdin`;
  - no flag and stdin is a terminal: `NoInput`.
- Reading:
  - `Stdin`: everything up to end of input. It must be valid UTF-8, with the same message
    as filter mode (`input is not valid UTF-8`).
  - `Clipboard` and `Primary`: `SelectionClipboard::read`. `Content::Text` is the input.
    `Content::Files` and `Content::Other` are errors (messages below).
- The output is `Engine::obfuscate(input)`, written to stdout. Nothing is added.
- `-n` removes exactly one trailing `\n`, or one trailing `\r\n`, from the output before it
  is written. Without a trailing line break it does nothing.
- A closed stdout (broken pipe) counts as success, as in filter mode.
- `filter.rs` may be refactored so that both share the reading and writing code. Filter
  mode's behaviour must not change.

### Exit codes and messages

Messages go to stderr with the prefix `sagiri-cli: `.

| Situation | Message | Exit |
|---|---|---|
| success | – | 0 |
| no `--in` and stdin is a terminal | `no input: pipe text into sagiri-cli or use --in clipboard` | 2 |
| invalid flag or flag value | clap's message | 2 |
| input is not valid UTF-8 | `input is not valid UTF-8` | 1 |
| selection holds files | `the clipboard holds files, not text` (or `the primary selection holds …`) | 1 |
| selection holds no text | `the clipboard holds no text` (or `the primary selection holds no text`) | 1 |
| clipboard cannot be opened or read | `cannot read the clipboard: <error from the clipboard crate>` | 1 |
| key or config error | as in `sagiri` | 1 |

On an error nothing is written to stdout.

### Documentation

- `README.md`: a `sagiri-cli` part in "Usage" with the command line, two examples
  (`sagiri-cli < input.txt`, `sagiri-cli --in clipboard`) and the exit codes. "Build and
  run" mentions `cargo run -p sagiri --bin sagiri-cli -- --help`.
- `CLAUDE.md`: the run command names the binary.

## Out of scope

- `--out` and writing to the clipboard (ticket 0017).
- File arguments (backlog).
- Any change to the `sagiri` binary. Its filter mode stays.
- Reading HTML or other rich formats from the clipboard.

## Design notes

- **Same bytes as filter mode.** `sagiri-cli < x` and `sagiri < x` must print the same
  bytes with the same key. Filter mode stays because `tickets/README.md` pins it and
  existing tests use it.
- **An explicit `--in stdin` reads a terminal** until end of input. Only the implicit
  default refuses a terminal, because a silently waiting program looks like a hang.
- **`-n` exists for the clipboard.** `echo text | …` adds a line break that is unwanted in
  a clipboard. It is opt-in, so the default stays byte-exact.
- **The clipboard is opened only when it is needed.** `SystemClipboard` is lazy; the
  stdin-to-stdout path must work on a machine without a display.
- **Tests never reach a real clipboard.** Unit tests use `FakeSelections`. The one
  integration test that selects the clipboard runs on Linux only and removes `DISPLAY` and
  `WAYLAND_DISPLAY` from the child's environment, so the open fails in a known way. On
  macOS the same test would read the real pasteboard; it must not exist there.
- A terminal on stdin cannot be produced with `assert_cmd`. The decision is therefore a
  pure function with its own tests.

## Acceptance criteria

Unit tests in `cli.rs`, with `FakeSelections` and an engine built from the test key:

- [ ] `flag_wins_over_terminal_check`.
- [ ] `stdin_is_default_when_piped`.
- [ ] `terminal_without_flag_is_no_input`.
- [ ] `clipboard_input_is_obfuscated`: with `server 172.55.0.1 is down` on the fake
      clipboard, the output equals `engine.obfuscate(..)` and does not contain
      `172.55.0.1`.
- [ ] `primary_input_is_obfuscated`: the same through `Selection::Primary`, with different
      text on the clipboard selection to prove which one was read.
- [ ] `selection_without_text_is_error` and `files_on_selection_is_error`, each for both
      selections, with the messages from the table.
- [ ] `error_messages_never_contain_content`: for invalid UTF-8 and for a failing fake, the
      message does not contain the input.
- [ ] `trim_newline_removes_exactly_one`: `a\n\n` gives `a\n`, `a\r\n` gives `a`, `a`
      stays `a`.
- [ ] `input_selection_is_not_written`: after a run with `--in clipboard`, the fake's write
      and clear counters are 0.

Integration tests in `crates/sagiri/tests/cli_tool.rs`:

- [ ] `cli_matches_filter_mode`: the same input and key file through `sagiri` and through
      `sagiri-cli` give identical stdout.
- [ ] `trim_newline_flag`: `-n` on input ending in a line break.
- [ ] `invalid_in_value_exits_2`.
- [ ] `invalid_utf8_exits_1`: stdout is empty.
- [ ] `help_and_version`: `--help` lists `--in` and `--trim-newline`; `--version` prints
      the package version.
- [ ] Linux only: `clipboard_without_display_exits_1`: `--in clipboard` without `DISPLAY`
      and `WAYLAND_DISPLAY` exits 1 with empty stdout and a message starting with
      `sagiri-cli: cannot read the clipboard`.

And:

- [ ] `cargo run -q -p sagiri -- --version` still runs the `sagiri` binary.
- [ ] `README.md` has the `sagiri-cli` part described above.
- [ ] `scripts/check.sh` passes.

Manual checks on X11 (listed in the report as done or not possible):

- [ ] `printf 'ssh admin@172.55.0.1' | xclip -selection clipboard`, then
      `sagiri-cli -i clipboard` prints the text with a different address.
- [ ] After selecting text with the mouse, `sagiri-cli -i primary` prints its obfuscated
      form.
- [ ] After copying a file in a file manager, `sagiri-cli -i clipboard` exits 1 with the
      "holds files" message.
- [ ] `sagiri-cli` alone in a terminal exits 2 at once.

## Test plan

- Unit tests in `cli.rs`; integration tests in `tests/cli_tool.rs` with the helpers pattern
  of `tests/filter.rs` (temporary `HOME`, `XDG_DATA_HOME`, `XDG_CONFIG_HOME`).
- The manual checklist. An implementer without a display states that in the report.
