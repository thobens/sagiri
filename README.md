# Sagiri

Sagiri obfuscates pasted text. It finds sensitive values such as IP addresses, URLs,
hostnames, email addresses, card numbers, IBANs and phone numbers, and replaces each with a
realistic-looking fake value. The same original always gets the same replacement, across
pastes and across sessions.

Status: pre-MVP.

## Build and run

```
cargo build
cargo run -p sagiri -- --help
cargo run -p sagiri --bin sagiri-cli -- --help
```

## Usage

```
sagiri < input.txt > output.txt
```

Text on stdin is written to stdout with sensitive values replaced. The same original gets
the same replacement on every run. Running `sagiri` with a terminal on stdin starts the
interactive UI: paste a text (terminal paste or `Ctrl+V`), see the obfuscated version next
to it, press `y` or `Enter` to copy it, `x` to clear, `q` to quit. Scrolling uses the arrow
keys, `hjkl`, `PgUp`, `PgDn`, `g` and `G`. Without a terminal on stdout, interactive mode
exits with status 2.

Clipboard note: on Linux the clipboard content is served by the Sagiri process that set it.
Without a clipboard manager, the copied text can disappear when Sagiri exits. Terminals
without bracketed paste deliver a paste as key presses that trigger key bindings; use
`Ctrl+V` there.

The secret key is stored in `$XDG_DATA_HOME/sagiri/key` (default
`~/.local/share/sagiri/key`) and is created on first use with mode 0600. `--key-file <PATH>`
or the `SAGIRI_KEY_FILE` environment variable selects another file; the flag wins.
Deleting the key file changes all replacements. Keep it private.

Exit codes: 0 success, 1 runtime error (for example input that is not valid UTF-8), 2 usage
error.

Known limitation: a bare hostname that ends in a top-level domain that is also a common
file extension or identifier in code (`.rs`, `.py`, `.md`, `.zip`, `.id`, `.name`, `.sh`
and similar) is not detected, so that `main.rs` and `user.name` stay untouched. The same
name inside a URL is still replaced.

### `sagiri-cli`

`sagiri-cli` never opens a UI. It reads text from the selected input, obfuscates it and
writes it to the selected output, with the same replacements as `sagiri`.

```
sagiri-cli [-i, --in <stdin|clipboard|primary>] [-o, --out <stdout|clipboard|primary>]
           [-n, --trim-newline] [--key-file <PATH>] [--config <PATH>]
```

```
sagiri-cli < input.txt
sagiri-cli --in clipboard
some-command | sagiri-cli -n --out clipboard
sagiri-cli --in clipboard --out clipboard
```

The last example obfuscates the clipboard in place.

- `--in stdin` (the default) reads stdin to its end. Without `--in`, a terminal on stdin is
  refused instead of waiting for input.
- `--in clipboard` reads the clipboard, `--in primary` the primary selection (Linux).
- `--out stdout` (the default) writes to stdout. `--out clipboard` and `--out primary` set
  the selection instead; nothing is written to stdout then.
- `-n` removes one trailing line break (`\n` or `\r\n`) from the output, for every output.
- `--key-file` and `--config` work as for `sagiri`.

On Linux a selection is served by the process that set it. With `--out clipboard` or
`--out primary`, `sagiri-cli` therefore leaves a small background process (`sagiri-cli
--hold …`) that holds the content until the next copy replaces it, and then exits. The text
reaches that process through a pipe, never as an argument or environment variable.

Exit codes: 0 success; 1 runtime error (input that is not valid UTF-8, a selection that
holds files or no text, a clipboard that cannot be read or written, a key or config error);
2 usage error (an invalid flag, or no `--in` with a terminal on stdin). On an error nothing
is written to stdout.

## Session mode

While a session is active, every text copied to the clipboard is replaced by its obfuscated
form, so that pasting it anywhere gives the obfuscated text. A session is switched on and off
with a hotkey that `sagiri-session` grabs itself, or with a command bound to a key in the
window manager. Both ways can be used side by side.

`sagiri-session run` starts the session process; it belongs in the login or window manager
startup. `sagiri-session toggle` switches the session on or off and belongs on a key. For
i3:

```
exec --no-startup-id sagiri-session run
bindsym $mod+Shift+o exec --no-startup-id sagiri-session toggle
```

`run` accepts `--active` (start the session right away), `--key-file` and `--config`. The
other commands talk to the running process and print its state afterwards, `active` or
`inactive`:

- `start` starts the session and prints `active`.
- `stop` stops the session and prints `inactive`.
- `toggle` switches the session and prints the new state.
- `status` changes nothing and prints the current state.
- `quit` ends the session process and prints `inactive`.

Without a running process these commands exit with status 1. `--socket <PATH>` selects
another control socket than `$XDG_RUNTIME_DIR/sagiri/session.sock`.

With `hotkey` set in the `[session]` table of the config file, `sagiri-session run` grabs
that key combination and the combination switches the session on and off:

```toml
[session]
hotkey = "ctrl+alt+o"
```

The `toggle` command keeps working while a hotkey is set. The syntax is described under
[Session settings](#session-settings).

- A combination that the window manager or another program already uses cannot be
  grabbed; `sagiri-session run` then exits with status 1 and says so.
- After a change of the keyboard layout, `sagiri-session run` has to be restarted.
- Holding the combination switches the session once.

Rules and limits:

- Copied text comes back as plain text; formatting is lost.
- Copied files and images are not changed. File names are not obfuscated.
- For a short moment after each copy the original is on the clipboard. A clipboard manager
  with a history may record it; its history should be paused or switched off while a session
  is used. Session mode prevents accidental pasting and does not protect against other
  programs on the same machine.
- Copying a part of an already obfuscated text obfuscates that part again.
- Linux with X11 only. Under Wayland, on macOS, or without `DISPLAY`, `run` exits with
  status 1 and names the reason.

On X11 a middle click pastes the PRIMARY selection, which is the text currently selected
with the mouse, not the clipboard. During a session a middle click therefore still pastes
the original. With `primary = "mirror"` in `[session]`, every text that the session writes
to the clipboard is written to PRIMARY as well, so after a copy both the paste shortcut and
a middle click give the obfuscated text. If PRIMARY cannot be written, the clipboard keeps
its obfuscated text and a notification says that a middle click may paste something else.
Two limits remain:

- Text that is selected but not copied is still pasted unchanged by a middle click.
- After a copy, the application that the text was copied from loses its visible selection,
  because PRIMARY now belongs to Sagiri.

`primary = "watch"` is experimental. Its costs come first:

- As soon as a selection is replaced, the application loses its visible selection: the
  highlight disappears.
- In terminals and other programs that copy "the current selection", a copy shortcut
  pressed after that copies nothing. With watch mode, select and middle-click; do not
  select and then copy.
- Selections made with the keyboard may be replaced before they are complete.
- The mode is experimental and may change or be removed.

With `primary = "watch"` in `[session]`, the session replaces the selected text itself, so
that selecting text with the mouse and middle-clicking somewhere else pastes the obfuscated
text without a copy in between. The selection is replaced once it has stopped changing:
150 ms after the last change while neither mouse button 1 nor Shift is held, and at the
latest 30 s after the selection started, even during a drag. Starting a session also
replaces what PRIMARY holds at that moment. In watch mode nothing is mirrored from the
clipboard to PRIMARY. If PRIMARY cannot be written, it is cleared and the clipboard is left
as it is.

How applications behaved in a test on X11 without a window manager (a Qt application was
not tested):

- xterm: the highlight disappears. xterm takes the selection when the button is released,
  so a drag is never interrupted.
- GTK3 (mousepad): the highlight disappears. GTK takes PRIMARY during the drag, but holding
  the button prevents the replacement; a 3 s drag over three lines was complete.
- Firefox: the highlight stays. Firefox takes the selection when the button is released, so
  a drag is not interrupted.

The session process shows a desktop notification when the session starts or stops, when a
selection cannot be replaced, and when the connection to the display is lost. Notifications
are shown through `notify-send` (package `libnotify` on Arch Linux). If `notify-send` is
missing, `run` prints one warning and keeps running without notifications. `notify = false`
in `[session]` switches them off. The same messages always go to the standard error of
`sagiri-session run` as well. Notifications never contain copied text.

## Configuration

An optional TOML file teaches Sagiri what it cannot detect on its own. The location is the
first of:

1. `--config <PATH>`
2. the `SAGIRI_CONFIG` environment variable, if set and not empty
3. `$XDG_CONFIG_HOME/sagiri/config.toml` (default `~/.config/sagiri/config.toml`)

A path given by flag or environment variable must exist. A missing file at the XDG path
means that all defaults apply. The same holds if the default location cannot be
determined because neither `XDG_CONFIG_HOME` nor `HOME` is usable.

```toml
# Values that are never replaced.
allow = ["intranet.acme.com", "*.github.com", "10.20.0.0/16", "192.0.2.7"]

# Literal terms that are always replaced.
terms = ["Acme Corp", "Project Falcon"]

# Rules are on unless switched off here.
[rules]
phone = false
```

- `allow` entries are exact values (case-insensitive; IP addresses compare as addresses),
  subdomain wildcards (`*.github.com` matches `api.github.com` but not `github.com`), or IP
  networks in CIDR notation. An allowed value is checked as a whole: `github.com` keeps
  `https://github.com/org/repo` but not `api.github.com`. In an email address an allowed
  domain is kept and the local part is still replaced.
- `terms` are matched literally, ignoring case, as whole words. They need at least two
  characters.
- `[rules]` accepts `term`, `url`, `email`, `ipv4`, `ipv6`, `hostname`, `payment_card`,
  `iban` and `phone`.
- Unknown keys are errors, so that a typo does not silently disable protection.

### Session settings

The `[session]` table is read by `sagiri-session` only. It has no effect on `sagiri` and
`sagiri-cli`.

```toml
[session]
hotkey = "ctrl+alt+o"
primary = "mirror"
notify = true
```

- `hotkey` (default: none) is a key combination that `sagiri-session` grabs to switch the
  session on and off (X11 only). Parts are joined with `+`; case and spaces around a part
  do not matter, so `Ctrl + Alt + O` is the same as `ctrl+alt+o`.
  - Modifiers: `ctrl` (or `control`), `alt`, `shift`, `super` (or `win`). At least one of
    `ctrl`, `alt` or `super` is required.
  - Exactly one key: a letter `a` to `z`, a digit `0` to `9`, `f1` to `f12`, `insert` or
    `pause`. Keys that need Shift or AltGr on the current keyboard layout cannot be used.
  - A wrong value makes `sagiri-session run` exit with status 1 on every platform, before
    the display is checked. A combination already used by the window manager cannot be
    grabbed and also ends `run` with a message. After a change of the keyboard layout,
    `sagiri-session run` has to be restarted. The `toggle` command keeps working either
    way.
- `primary` (default: `"off"`) sets how the PRIMARY selection is treated: `"off"`,
  `"mirror"` or `"watch"`.
  - `"off"` leaves PRIMARY alone; a middle click after a copy pastes the original selection.
  - `"mirror"` writes every text that the session puts on the clipboard to PRIMARY too, so
    that a middle click after a copy pastes the obfuscated text. Text that is selected but
    not copied is still pasted unchanged by a middle click. After a copy, the application
    that the text was copied from loses its visible selection, because PRIMARY now belongs
    to Sagiri.
  - `"watch"` (experimental) replaces selected text on PRIMARY itself, without a copy.
    The selection loses its highlight and can no longer be copied afterwards; see
    [Session mode](#session-mode) for all costs.
- `notify` (default: `true`) switches desktop notifications on or off.

Warning: the config file contains the sensitive terms in plain text. Do not share it and do
not commit it to a repository.

## Checks

```
scripts/check.sh
```

This runs rustfmt, clippy with warnings as errors, and all tests. CI runs `scripts/check.sh`
on Linux and macOS.

`scripts/x11-test.sh` runs the X11 integration tests of session mode on Linux. They start
the real `sagiri-session` against a virtual X server and check clipboard replacement and the
hotkey. The script needs `xvfb-run`, which comes with the X server's Xvfb package, and
`xdotool`; it exits 2 if one of them is missing. It is not part of `scripts/check.sh` and
does not run in CI, so it has to be run by hand.

## More

- `PLAN.md`: how the MVP is planned.
- `tickets/README.md`: shared rules, architecture and the ticket index.
