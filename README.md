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

The `Makefile` (GNU Make) wraps the release build and the installation:

```
make                              # release build of sagiri, sagiri-cli and sagiri-session
make sagiri-cli                   # release build of one binary
make install PREFIX=~/.local USERUNITDIR=~/.local/share/systemd/user
                                  # copy the release binaries to ~/.local/bin (and the
                                  # systemd user unit on Linux, see Installing)
sudo make install                 # copy them to /usr/local/bin
make check                        # run scripts/check.sh
```

`make install` does not build; run `make` first. `make help` lists all targets and
variables.

## Installing

### From a release

Each release on GitHub has one archive per platform and a file `SHA256SUMS`:

- `sagiri-<version>-x86_64-unknown-linux-gnu.tar.gz`
- `sagiri-<version>-aarch64-unknown-linux-gnu.tar.gz`
- `sagiri-<version>-aarch64-apple-darwin.tar.gz`

The Linux archives are built on Ubuntu 24.04 and need a glibc at least that new. The macOS
archive is for Apple silicon. On macOS `sagiri-session` only says that session mode is not
supported there.

Download the archive for the platform and `SHA256SUMS`, then check the checksum and the
origin. `gh release verify` checks the release attestation that GitHub records for an
immutable release; `gh attestation verify` checks that the archive was built by this
repository's release workflow from the tagged commit:

```
sha256sum -c --ignore-missing SHA256SUMS
gh release verify v<version> --repo thobens/sagiri
gh attestation verify sagiri-<version>-<target>.tar.gz --repo thobens/sagiri
```

Unpack the archive and copy `bin/*` to a directory on the `PATH`:

```
tar -xzf sagiri-<version>-<target>.tar.gz
install -m 755 sagiri-<version>-<target>/bin/* ~/.local/bin/
                                  # or: sudo install -m 755 ... /usr/local/bin/
```

On Linux the archive also contains the systemd user unit
`lib/systemd/user/sagiri-session.service`. Its `ExecStart=` points to
`/usr/local/bin/sagiri-session`. Copy it as described in
[Starting with systemd](#starting-with-systemd).

### From source

Build first and then copy the three binaries to `$PREFIX/bin`:

```
make release
make install PREFIX=~/.local USERUNITDIR=~/.local/share/systemd/user
                                  # or: sudo make install PREFIX=/usr/local
```

`make uninstall` with the same `PREFIX` and `USERUNITDIR` removes them again.

On Linux, `make install` also installs the systemd user unit `sagiri-session.service` to
`$USERUNITDIR` (default `$PREFIX/lib/systemd/user`); see
[Starting with systemd](#starting-with-systemd). An install into the home directory needs
`USERUNITDIR=~/.local/share/systemd/user`, as in the example above, because
`~/.local/lib/systemd/user` is not on the search path of the user manager.

On Arch Linux and its derivatives, `make pkg` builds a package from the local checkout with
`makepkg` (`packaging/arch/PKGBUILD`). It runs the tests and leaves the package in
`packaging/arch`. `pacman` then installs and removes it like any other package:

```
make pkg
sudo pacman -U packaging/arch/sagiri-*.pkg.tar.zst
sudo pacman -R sagiri
```

The package installs `sagiri`, `sagiri-cli` and `sagiri-session` to `/usr/bin` and the
systemd user unit to `/usr/lib/systemd/user/sagiri-session.service`. Neither `make install`
nor the package enables the unit. The optional dependency `libnotify` provides the desktop
notifications of session mode.

To start session mode at login with systemd, see
[Starting with systemd](#starting-with-systemd). Without systemd, add the two lines from
[Session mode](#session-mode) to the i3 config:

```
exec --no-startup-id sagiri-session run
bindsym $mod+Shift+o exec --no-startup-id sagiri-session toggle
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

### Starting with systemd

On Linux, `make install` and the Arch package install the systemd user unit
`sagiri-session.service`. It runs `sagiri-session run` with the defaults, restarts it after
a failure and stops it at logout.

The Linux archive of a release contains the unit as
`lib/systemd/user/sagiri-session.service`, with `ExecStart=/usr/local/bin/sagiri-session run`.
It is copied to `~/.config/systemd/user/` (one user) or `/etc/systemd/user/` (all users);
with the binaries in another directory, `ExecStart=` in the copy is changed to match. Then
`systemctl --user daemon-reload` loads it:

```
install -Dm644 sagiri-<version>-<target>/lib/systemd/user/sagiri-session.service \
    ~/.config/systemd/user/sagiri-session.service
systemctl --user daemon-reload
```

Enabling and starting it:

```
systemctl --user enable --now sagiri-session.service
```

`systemctl --user disable --now sagiri-session.service` stops and disables it again.

- The unit is started by `graphical-session.target`. A window manager session that never
  reaches that target (plain i3 started from a display manager, for example) starts the
  unit from its config instead:

  ```
  exec --no-startup-id systemctl --user start sagiri-session.service
  ```

- The user manager needs `DISPLAY` and `XAUTHORITY` in its environment. Most distributions
  import them at X start-up. Where they are missing, this line in the session start-up
  imports them:

  ```
  dbus-update-activation-environment --systemd DISPLAY XAUTHORITY
  ```

  Without `DISPLAY`, or with `WAYLAND_DISPLAY` set, the unit is skipped and not started.
- The unit replaces the line `exec --no-startup-id sagiri-session run`. With both in place
  the second process finds the first one running and exits with an error. The `bindsym`
  line for `sagiri-session toggle` stays.
- After a change of the config file or of the keyboard layout:
  `systemctl --user restart sagiri-session.service`.
- To start a session at login, a drop-in created with
  `systemctl --user edit sagiri-session.service` clears `ExecStart=` and sets it again with
  `--active` (the path is the one in the installed unit, `systemctl --user cat
  sagiri-session.service` shows it):

  ```ini
  [Service]
  ExecStart=
  ExecStart=/usr/bin/sagiri-session run --active
  ```

- `journalctl --user -u sagiri-session.service` shows the messages of the process. They
  never contain copied text.

A wrong `hotkey`, a combination that is already grabbed, or a second session process makes
the process exit with status 1. systemd restarts it 2 s after each failure, at most five
starts within 30 s, and then leaves the unit in the state `failed`; the journal names the
reason. The limit also applies to manual starts, so after the cause is fixed the counter is
cleared first:

```
systemctl --user reset-failed sagiri-session.service
systemctl --user restart sagiri-session.service
```

The i3 lines above remain the way to start session mode without systemd.

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
  and at most 1000 characters.
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

This runs rustfmt, clippy with warnings as errors, and all tests, including the examples in
the documentation of `sagiri-core`. It also builds that documentation with warnings as errors;
`cargo doc -p sagiri-core --no-deps --open` shows it. CI runs `scripts/check.sh` on Linux and
macOS.

`scripts/x11-test.sh` runs the X11 integration tests of session mode on Linux. They start
the real `sagiri-session` against a virtual X server and check clipboard replacement and the
hotkey. The script needs `xvfb-run`, which comes with the X server's Xvfb package,
`xdotool`, `xclip` and `xsel`; it exits 2 if one of them is missing. It unsets
`WAYLAND_DISPLAY`, so it also works from a Wayland session. The tests refuse to run unless
the script has set `SAGIRI_X11_TEST`, so that `cargo test -- --ignored` never takes over the
real clipboard or keyboard. The script is not part of `scripts/check.sh` and does not run in
CI, so it has to be run by hand.

`scripts/audit.sh` (or `make audit`) checks the built binaries against the RustSec
advisory database. Without arguments it builds `sagiri`, `sagiri-cli` and `sagiri-session`
in release mode with `cargo auditable`, through `make release CARGO='cargo auditable'`, and
then runs `cargo audit bin` on them. `cargo auditable` embeds the list of linked crates in
each binary, so the check covers exactly what is in the binary: development dependencies
and crates for other platforms do not count. With arguments, the script audits the
binaries at the given paths and builds nothing.

The script needs two tools; `cargo-auditable` only when it builds. Both are installed
with:

```
cargo install --locked cargo-audit cargo-auditable
```

The versions CI uses are the `_VERSION` variables at the top of
`.github/workflows/audit.yml`; appending `@<version>` to each name installs the same ones.

It needs network access, because it downloads the advisory database. Exit status:

- 0: no binary has a known vulnerability.
- 1: at least one binary has a known vulnerability.
- 2: `cargo-audit` is missing, `cargo-auditable` is missing and the script has to build
  (no arguments), or an argument is not a file.
- 3: the check is incomplete, for example because a binary was built without
  `cargo auditable` or the database could not be fetched.

Advisories of the kinds "unmaintained" and "yanked" are printed as warnings and do not
change the exit status.

Its result changes when a new advisory is published, so it is not part of
`scripts/check.sh`. CI runs it on Linux and macOS once a day, on every push to `master` and
on pull requests that change `Cargo.lock`, a `Cargo.toml`, the Makefile, the script or its
workflow (`.github/workflows/audit.yml`).

## Releasing

A tag `v<version>` on `master` starts `.github/workflows/release.yml`. It runs
`scripts/check.sh`, builds the three archives natively on Linux x86_64, Linux arm64 and
macOS arm64 with `cargo auditable`, checks the binaries with `scripts/audit.sh`, attests the
build provenance of each archive and publishes the release with the archives and
`SHA256SUMS`. A manual run (`workflow_dispatch`) is a dry run: it builds the same archives
as workflow artifacts and publishes nothing.

1. Set the version in the workspace `Cargo.toml` and `pkgver` in `packaging/arch/PKGBUILD`,
   with `pkgrel=1`.
2. Run `cargo build` once, so that `Cargo.lock` follows.
3. Merge to `master`.
4. Create the tag `v<version>` on that commit and push the tag:

   ```
   git tag v<version>
   git push origin v<version>
   ```

The workflow fails unless the tag is `v` followed by the workspace version and the tagged
commit is on `master` (`scripts/release-tag-check.sh <tag>`). `scripts/package.sh <target>`
packs the release binaries that are already built into `dist/`.

A published release cannot be changed or repeated: its tag and assets are fixed. A mistake
needs a new version.

## Dependency updates

Renovate, as the hosted Mend Renovate GitHub App, opens one pull request per update. Its
configuration is `.github/renovate.json5`. It covers the crates in the `Cargo.toml` files
and `Cargo.lock`, the actions in the workflows, which stay pinned to a commit SHA, and the
`*_VERSION` variables in the workflows, each of which has a `# renovate:` comment line above
it that names the crate.

- A patch update of any version, and a minor update of a version from 1.0 on (`4.6` to
  `4.7`), gets its pull request seven days after the release. Before that, Renovate creates
  neither a branch nor a pull request. Renovate merges the pull request itself once all
  checks on it have passed.
- A major update, and a minor update of a `0.x` version (`0.30` to `0.31`, which Cargo
  treats as incompatible), gets its pull request at once and is merged by a person.
- Every other kind of update, for example a new commit SHA for an unchanged action
  version, also gets its pull request at once and is merged by a person.

The checks include `scripts/check.sh`, and with it the golden vector tests, and the audit
for pull requests that change `Cargo.lock`. The golden vector tests are what keeps an
automatic update from changing replacements: an update that changes any output fails
them. A pull request with a failing check is not merged automatically and stays open for a
person. Such a pull request is never fixed by changing a golden vector.

A version in `Cargo.toml` is raised together with `Cargo.lock`. Lock file maintenance is
off. The Dependency Dashboard issue lists the updates that are waiting for their seven
days. The pull requests for `x11rb` and `signal-hook` carry a note: `cargo tree -p sagiri
-i <crate>` must show one version after the update.

## More

- `PLAN.md`: how the MVP is planned.
- `tickets/README.md`: shared rules, architecture and the ticket index.

## License

Sagiri is licensed under either of the following, at the user's choice:

- Apache License, Version 2.0 (`LICENSE-APACHE`)
- MIT license (`LICENSE-MIT`)
