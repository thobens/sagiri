# Pass 1: where data can leave

Covers G1 (content), G2 (key), G6 (failures) and G7 (local only) of `guarantees.md`.

The method is the same for every channel: list the sinks, then follow each value that
reaches a sink back to where it comes from. A sink is safe when everything that reaches it
is a fixed text, a path, a key name, an index or a number. It is a finding when pasted
text, a `terms` or `allow` entry, or key material can reach it, even on a path that only
an error takes.

`scripts/sinks.sh` lists the sinks by category. It finds candidates; reading decides.

## Contents

- [stdout and stderr](#stdout-and-stderr)
- [Panics](#panics)
- [Debug and Display](#debug-and-display)
- [Errors from dependencies](#errors-from-dependencies)
- [Files](#files)
- [Child processes](#child-processes)
- [Control socket](#control-socket)
- [Selections and X11](#selections-and-x11)
- [Terminal](#terminal)
- [Memory](#memory)
- [Network](#network)
- [Key file](#key-file)
- [Tests, examples and the repository](#tests-examples-and-the-repository)
- [Checking a channel by running the program](#checking-a-channel-by-running-the-program)

## stdout and stderr

`sinks.sh output`

- stdout carries the obfuscated text and, for `sagiri-session`, a state word. Nothing else.
  Check that no path writes the *input* to stdout: an early return, a fallback, a
  "nothing found" shortcut.
- stderr carries messages. For each one, read every value that is formatted into it.
  `{e:#}` prints an `anyhow` chain: every error in the chain counts, including the ones
  that come from dependencies.
- stderr of `sagiri-session run` goes to the systemd journal when the unit is used. The
  journal is persistent and readable by more than the session. Anything on that stderr is
  written to disk.
- `sagiri-core` denies `clippy::print_stdout` and `clippy::print_stderr`. Check that the
  attribute is still in `lib.rs` and that nothing allows it locally.

## Panics

`sinks.sh panic slice arith`

A panic message goes to stderr through the default panic hook. Several kinds of panic
quote the data they failed on:

- **Slicing a `str`.** `&text[a..b]` with an index that is not on a character boundary, or
  out of range, panics with a message that quotes the string. This is the main risk in the
  rules: they work with byte offsets on pasted text. For each `slice` hit on a `str` that
  comes from input, establish why both ends are character boundaries (they come from a
  regex match, from `char_indices`, from a search for an ASCII byte) and why they are in
  range. Slicing `&[u8]` out of range prints only numbers.
- **`unwrap` and `expect` on a `Result`.** The message contains the `Debug` form of the
  error. `FromUtf8Error` holds the bytes, a `toml` error holds a piece of the file, an
  error type of the project may hold anything.
- **`assert_eq!` and `assert_ne!`** print both sides.
- **A panic message with a formatted value**, as in `panic!("unexpected {x}")`.

Arithmetic can panic too, without quoting anything: a shift by an amount that is computed
and too large, an overflow in a debug build, a division by zero. Check the `arith` hits
whose operands depend on input.

Then check who installs a panic hook that withholds the message. At commit `43cb29b` only
the TUI does (`tui/mod.rs`, `install_panic_hook`). Filter mode, `sagiri-cli`, the holder
process and every thread of `sagiri-session` use the default hook. So for those the
question is whether a panic that quotes content is reachable at all, and a reachable one
is a G1 finding and, in session mode, a G6 finding as well.

A panic is also a failure mode, apart from its message:

- In `sagiri-session`, the main thread owns the session. If it panics while replacing a
  copy, the original stays on the clipboard and the process is gone. systemd restarts it
  inactive.
- A panic in a thread other than main ends only that thread. Check what the rest of the
  process does when the watcher, the control thread or the tray thread is gone, and
  whether the user learns that copies are no longer replaced.

## Debug and Display

`sinks.sh debug`

- A type that holds content or key material must not derive `Debug`. The project writes
  these impls by hand: `Key`, `Finding`, `Rendered`, `Allowlist`, `Content`. Check every
  derive on a type with a `String`, `Vec<u8>`, `Vec<String>` or another such type inside.
  `Config` derives `Debug` and holds `terms` and `allow`: find out whether anything
  formats it.
- A derived `Debug` is a leak waiting for one `{:?}`: in an error context, in an
  `expect`, in a test that prints on failure in CI. Report a derive on a content type even
  if nothing prints it today, as low severity, unless a test pins that nothing does.
- `{:?}` on a `&str`, a `PathBuf` or an `OsString` prints the value. Paths are allowed;
  content is not.
- `Display` impls of error types: every field that is formatted.

## Errors from dependencies

Errors of other crates often quote their input. Find where such an error can be created
from content and follow it to where it is printed or dropped.

| Source | What its message can hold |
|---|---|
| `toml` | A snippet of the file around the error. The config file holds `terms`. `config.rs` takes only `message()` and a position, and rewrites the rest: check that it still does, and that `message()` itself cannot quote a value (it names keys; `unknown field` names the field the user typed) |
| `regex` | The pattern. `TermRule` builds patterns from terms and maps the build error to a fixed text: check that it still does |
| `String::from_utf8` | `FromUtf8Error` holds the bytes in `Debug`; its `Display` does not |
| `clap` | Echoes an invalid argument. Arguments are flags and paths, never content: check that no flag takes content as its value |
| `std::io::Error` | An OS message; a path only when the code adds one |
| `arboard`, `x11rb` | Read the variants that can occur. An error that carries the selection's content would be a finding |
| `std::num::ParseIntError` and similar | No input |

## Files

`sinks.sh file`

- Sagiri writes exactly these: the key file, the directories for it, the control socket
  and its directory. Anything else is a finding: a temporary file, a cache, a history, a
  log, a state file.
- No original and no mapping is ever stored. A cache from original to replacement, even
  in memory across pastes, breaks the "derivation, not storage" decision. `sinks.sh state`
  shows statics and collections; check what each one holds and for how long.
- Shell scripts in `scripts/` and the workflows: they handle build output, not user data.
  Check that none of them reads a key file, a config file or a clipboard.

## Child processes

`sinks.sh process`

Arguments and environment of a process are visible to every user of the machine through
`/proc/<pid>/cmdline`, and to the same user through `/proc/<pid>/environ`.

- `sagiri-cli --hold <selection>`: the text goes through a pipe. Check that nothing was
  added to the command line or the environment, that stderr of the holder is discarded,
  and what the holder does with the text when taking the selection fails.
- `notify-send`: the arguments come from a fixed table per `Notice`. `Notice` has no
  variant that carries text; check that it stays that way. A new variant with a `String`
  is a finding.
- The program name `notify-send` is resolved through `PATH`. That is same-user territory
  and out of scope, but an absolute path in a system unit would be a hardening note.
- Any new `Command::new` needs the same three questions: what is on the command line, what
  is in the environment, where do its stdout and stderr go.

## Control socket

`session/control.rs`

- Directory mode 0700, socket mode 0600. `UnixListener::bind` creates the socket with the
  permissions the umask allows, and the mode is tightened afterwards: check the window,
  and check what protects the socket when `--socket` points into a directory that other
  users can enter (the 0700 applies only to a directory Sagiri creates).
- The protocol: one line in, one of five words; one line out. Check the length limit of a
  request, the read timeout, and that the answer never echoes the request.
- What a connection can do matters more than what it can read: `stop` and `quit` end the
  protection silently for whoever is at the keyboard. A user who can connect can make the
  victim paste originals. Check who can connect (A5).
- A stale socket is removed and replaced. Check that the code cannot be made to remove or
  replace a file that is not its own socket (symlink, race between check and use).

## Selections and X11

`clipboard.rs`, `clipboard/targets.rs`, `session/core.rs`, `session/x11.rs`, `holder.rs`

The product decisions for session mode are in `tickets/README.md` ("Session mode rules").
Each of them is a security property; check them one by one:

- Text is written back as plain text only. No rich target (HTML, RTF, URI list) of the
  original owner survives, because it can carry the original.
- Files, images and other content are left alone, and reading a selection never converts
  such content into text that is then written back or reported.
- Starting a session replaces what is on the clipboard at that moment.
- Fail closed: a failed write clears the selection and notifies; a failed clear notifies
  that the original is still there. Follow every `Err` in `on_change` and in the mirror
  and watch paths to one of these two outcomes.
- The session recognises its own writes (`last_written`). Check what it stores (the
  obfuscated text only), and whether a text can be crafted that the session mistakes for
  its own and therefore skips.
- Mirror and watch: every path that writes the clipboard also handles PRIMARY as the mode
  says, and a failure there is reported.
- Large content: how a long text is transferred, and what happens when the transfer or
  the obfuscation takes long. While `obfuscate` runs, the original is what a paste gets.
  Input that makes it slow (see "Cost" in `inference.md`) widens that window on purpose.
- At exit: what happens to a selection the process still owns. A handover to a clipboard
  manager must hand over the obfuscated text.

Never test these against the real display. `scripts/x11-test.sh` runs the X11 tests
against a virtual server.

## Terminal

`tui/`

- Pasted text is drawn on the screen. Control characters in it must not reach the
  terminal: an escape sequence in a paste can move the cursor, change the title, or write
  to the clipboard (OSC 52). `ui.rs` has `sanitize`; check that every string that comes
  from the buffer, a finding, an original or a replacement passes through it, including
  new widgets, status lines and error lines.
- Characters that change how text is displayed without being control characters:
  bidirectional overrides, zero-width characters, wide and combining characters. They can
  make the review screen show something other than what is copied.
- On exit and on a panic the terminal is restored and the alternate screen is left, so the
  original does not stay in the scrollback.
- Bracketed paste: without it a paste arrives as key presses that trigger key bindings.
  Check what a crafted paste can do through the bindings (copy the *original*, quit).
- Filter mode and `sagiri-cli` pass control characters through unchanged. That is by
  design: they are filters.

## Memory

`sinks.sh state`

- The key: `Key` and `Pseudonymizer` hold it in `Zeroizing`. Look for copies that are not:
  a plain array, a `Vec`, a `String` with hex, a value moved out of a `Zeroizing`. `hmac`
  and `sha2` are built with their `zeroize` feature; check `Cargo.toml` still says so.
- `DetStream` holds a block of HMAC output, not the key.
- Pasted text is not zeroized. That is an accepted limit. A finding would be content kept
  *longer* than the operation needs: a history, a global, a leaked allocation.

## Network

`sinks.sh net`

- `std::net` is used for address types only. Any socket other than the Unix control
  socket is a finding.
- Dependencies: `cargo tree -p sagiri -e normal --locked` must not contain an HTTP client,
  a TLS library, a DNS resolver or an async runtime. Compare with the crate table in
  `tickets/README.md`.
- `psl` has the public suffix list compiled in. Check that no feature fetches it.
- An X display with a host part (`DISPLAY=host:0`) is a TCP connection that carries
  selections in the clear. That is the user's configuration, not Sagiri's choice; a note,
  not a finding, unless Sagiri sets it.

## Key file

`key.rs`, `paths.rs`, `setup.rs`

- Creation: random bytes from `getrandom`, a private temporary file (`create_new`, 0600),
  published by a hard link, temporary file removed. Check each step for a window in which
  the key is readable by others or half written, and what a failure leaves behind.
- Loading: the permission check is done on the opened file, not on the path. A file that
  group or others can access is refused. Check that there is no way around the check
  (another function that loads a key, a flag, an environment variable).
- Path resolution: flag, then `SAGIRI_KEY_FILE`, then the XDG path. A relative or empty
  `XDG_*` value is ignored. Check that a key file is never created in a place the user did
  not expect (the current directory, `/tmp`), for every combination of unset variables.
- Messages name the key file's path and never its content, also for a malformed file.

## Tests, examples and the repository

- Test data is fictional: documentation IP ranges, published test card numbers, made-up
  names. A real-looking hostname, address, name or number in a test, a doc example or a
  ticket is a finding. `git log -p` matters too: a value removed from the tree is still
  in the history.
- The only key in the repository is the test key `00 01 .. 1f`.
- Nothing that `scripts/check.sh` runs touches a real clipboard, display or notification
  daemon.
- A test that prints content on failure prints it into a CI log. With fictional data that
  is harmless; note it only where a test reads something from the environment.

## Checking a channel by running the program

Reading shows what the code can do. Running shows what it did. Use a canary: a string that
appears nowhere else, placed in the input, and check where it turns up.

`scripts/probe.sh` runs the filter in an emptied environment with a temporary home and
reports stderr and every file the run left behind. Put the canary into the probe text, or
into a term of a `--config` file, and provoke errors:

```bash
S=.claude/skills/sagiri-security-review/scripts
T="$(mktemp -d)"

# Error path: invalid UTF-8 after the canary. stderr must not contain CANARY-7f3a.
printf 'CANARY-7f3a 192.0.2.7 \xff\n' | $S/probe.sh

# Config errors: a broken file that holds a term. stderr must name the key or the
# position, not the term.
printf 'terms = ["CANARY-7f3a", 5]\n' >"$T/bad.toml"
echo x | $S/probe.sh --config "$T/bad.toml"
```

Where `strace` is available, it shows every file, process and socket a run touches. Use it
on a binary that was started the way `probe.sh` starts it, never with the real key or
config:

```bash
strace -f -e trace=execve,openat,connect,sendto -o "$T/trace" <command>
```

A guarantee that only reading supports is weaker than one a test pins. For every channel
in the scope, look for the test that would fail if the channel started to carry content
(examples at `43cb29b`: `rejects_malformed_files_without_leaking_content`,
`error_messages_never_contain_content`, `content_debug_hides_text`). A channel without such
a test is a finding of its own: the guarantee holds today and nothing keeps it.
