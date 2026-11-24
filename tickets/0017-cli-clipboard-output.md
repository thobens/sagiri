---
id: "0017"
title: "sagiri-cli: clipboard output"
status: done
depends_on: ["0016"]
---

# 0017 — `sagiri-cli`: clipboard output

## Why

With clipboard output, `sagiri-cli --in clipboard --out clipboard` obfuscates the clipboard
in place, and `some-command | sagiri-cli --out clipboard` makes a result ready to paste.
On Linux this is harder than it sounds: the clipboard content is served by the process that
set it and is gone when that process exits. This ticket adds the output option and a small
holder process that keeps the content alive.

## Scope

In `crates/sagiri`.

### Command line

```
sagiri-cli [-i, --in <stdin|clipboard|primary>] [-o, --out <stdout|clipboard|primary>]
           [-n, --trim-newline] [--key-file <PATH>] [--config <PATH>]
```

- `--out` defaults to `stdout`. `pub enum Sink { Stdout, Clipboard, Primary }` in `cli.rs`.
- With a selection as the sink, nothing is written to stdout.
- `-n` applies to every sink.
- New rows for the table of ticket 0016:

  | Situation | Message | Exit |
  |---|---|---|
  | selection cannot be written | `cannot write the clipboard: <error from the clipboard crate>` | 1 |
  | the holder did not confirm | `the clipboard could not be set` | 1 |

### Writing a selection

- On Linux, through the holder (below).
- On other platforms, `SelectionClipboard::write_text` directly. The system keeps the
  content there.
- The choice is made with `cfg!(target_os = "linux")`, so that both paths compile
  everywhere.
- The logic in `cli.rs` receives the selection writer as a parameter (a function from
  `Selection` and text to a result). The binary passes the holder or the direct write;
  unit tests pass a recorder. No unit test starts a holder process through `cli.rs`.

### The holder (`src/holder.rs`)

`sagiri-cli` starts a second `sagiri-cli` process that owns the selection and stays until
another program takes it over.

- Hidden flag `--hold <clipboard|primary>`. It does not appear in `--help`. With it, the
  process is a holder and ignores every other option.
- The holder:
  1. reads the text from its stdin up to end of input;
  2. takes the selection with that text;
  3. writes the line `ok` to its stdout and closes stdout;
  4. serves the selection until another owner replaces it, then exits 0.
  If step 1 or 2 fails, it exits 1 without writing `ok`.
- The parent:
  1. starts `std::env::current_exe()` with only `--hold <selection>` as arguments, stdin
     and stdout piped, stderr discarded, in its own process group
     (`std::os::unix::process::CommandExt::process_group(0)`);
  2. writes the text to the child's stdin and closes it;
  3. reads one line from the child's stdout. `ok` means success; end of input or anything
     else is the error `the clipboard could not be set`;
  4. returns without waiting for the child.
- Functions:
  - `pub fn command(exe: &Path, selection: Selection) -> std::process::Command`
  - `pub fn spawn_with(command: Command, text: &str) -> anyhow::Result<()>`: steps 2 to 4,
    testable with any command
  - `pub fn spawn(selection: Selection, text: &str) -> anyhow::Result<()>`
  - `pub fn serve(input: impl Read, ack: impl Write, hold: &mut dyn Hold, selection:
    Selection) -> anyhow::Result<()>` with a small trait `Hold` (take the selection; serve
    until replaced), a system implementation on arboard and a fake for tests.

### Documentation

- `README.md`: `--out` in the `sagiri-cli` part, the in-place example, and a note that on
  Linux a small background process holds the content until the next copy.

## Out of scope

- A time limit after which the content is removed.
- Clearing the clipboard, clipboard history handling.
- Session mode (tickets 0019 onward).
- Changing how the TUI keeps its clipboard content.

## Design notes

- **The text travels through a pipe.** Never as an argument or an environment variable:
  both are readable by other processes on the machine.
- **stderr of the holder is discarded.** A caller that captures output, for example
  `x=$(… | sagiri-cli -o clipboard 2>&1)`, would otherwise wait until the holder exits.
- **The acknowledgement closes a gap.** Without it the parent could exit before the holder
  owns the selection, and a paste right after the command would get the old content.
- **No `unsafe`.** `fork`, `setsid` and `pre_exec` are not available under the workspace
  lint. Starting the same executable again with `process_group(0)` is the safe equivalent.
- **Blocking until replaced.** arboard's `SetExtLinux::wait()` blocks until another owner
  takes the selection, but only returns then, which is too late for the acknowledgement.
  The intended sequence is: a normal write (ownership is taken and served by arboard's own
  thread), then `ok`, then the waiting write of the same text. Setting a selection that the
  process already owns must not end the wait. Check this against the arboard 3.7
  documentation and its `examples/daemonize.rs`; if arboard offers a direct way to signal
  that ownership was taken, use that instead and say so in the report.
- **The system `Hold` cannot be tested automatically** (no real clipboard in tests). It
  stays a few lines; `serve` and `spawn_with` hold the logic.
- `spawn_with` tests use `sh -c '…'` as the child. `sh` exists on every supported platform.

## Acceptance criteria

Unit tests in `cli.rs`:

- [ ] `out_stdout_is_default`: the fake's write counter stays 0.
- [ ] `out_selection_writes_obfuscated_text`: for both selections, the injected writer
      receives exactly `engine.obfuscate(..)` and stdout stays empty.
- [ ] `clipboard_to_clipboard_replaces_in_place`.
- [ ] `trim_newline_applies_to_selection_output`.

Unit tests in `holder.rs`:

- [ ] `serve_acks_after_taking_the_selection`: with the fake, `ok\n` is written after the
      take and before the serve call.
- [ ] `serve_gives_no_ack_when_take_fails`: the result is an error and nothing is written.
- [ ] `serve_rejects_invalid_utf8`.
- [ ] `spawn_succeeds_on_ack`: child `sh -c 'cat >/dev/null; echo ok'`.
- [ ] `spawn_fails_when_child_exits_without_ack`: child `sh -c 'cat >/dev/null'`.
- [ ] `spawn_does_not_wait_for_the_child`: child `sh -c 'cat >/dev/null; echo ok; sleep 5'`
      returns in under one second.
- [ ] `command_carries_no_text`: the arguments of `command(..)` are exactly `--hold` and
      the selection name; no environment variable is added.

Integration tests in `crates/sagiri/tests/cli_tool.rs`:

- [ ] `hold_flag_is_hidden_from_help`.
- [ ] `invalid_out_value_exits_2`.
- [ ] Linux only: `out_clipboard_without_display_exits_1`: without `DISPLAY` and
      `WAYLAND_DISPLAY` the command exits 1 with `sagiri-cli: the clipboard could not be
      set` and leaves no child process waiting.

And:

- [ ] `README.md` documents `--out` as described.
- [ ] `scripts/check.sh` passes.

Manual checks on X11 (listed in the report as done or not possible):

- [ ] `echo 'host 172.55.0.1' | sagiri-cli -n -o clipboard` returns at once;
      `xclip -o -selection clipboard` prints the obfuscated text without a line break.
- [ ] `pgrep -f 'sagiri-cli --hold'` shows one process; after copying something else it is
      gone.
- [ ] `sagiri-cli -i clipboard -o clipboard` after copying a text: pasting gives the
      obfuscated text.
- [ ] `sagiri-cli -o primary`, then a middle click pastes the obfuscated text.
- [ ] `x=$(echo a | sagiri-cli -o clipboard 2>&1); echo done` prints `done` at once.

## Test plan

- Unit tests in `cli.rs` and `holder.rs`; integration tests in `tests/cli_tool.rs`.
- The manual checklist. An implementer without a display states that in the report.
