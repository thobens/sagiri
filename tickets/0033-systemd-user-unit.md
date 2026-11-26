---
id: "0033"
title: systemd user unit for `sagiri-session`
status: done
depends_on: ["0027", "0029"]
---

# 0033 — systemd user unit for `sagiri-session`

## Why

`sagiri-session run` has to be started by hand or from the window manager's config, and
nothing restarts it when the connection to the display is lost. A systemd user unit starts
the session process with the graphical session, restarts it after a failure, stops it at
logout and collects its messages in the journal.

## Scope

### `packaging/systemd/sagiri-session.service.in`

A template for a **user** unit (`systemctl --user`). The placeholder `@BINDIR@` is replaced
at install time.

```ini
[Unit]
Description=Sagiri session mode: obfuscates copied text while a session is active
Documentation=https://github.com/thobens/sagiri
PartOf=graphical-session.target
After=graphical-session.target
ConditionEnvironment=DISPLAY
ConditionEnvironment=!WAYLAND_DISPLAY

[Service]
ExecStart=@BINDIR@/sagiri-session run
Restart=on-failure
RestartSec=2
NoNewPrivileges=yes

[Install]
WantedBy=graphical-session.target
```

- `Documentation=` is the repository's web address, as in `packaging/arch/PKGBUILD`.
- No `--active`, no `--socket`, no `--key-file`, no `--config`: the unit runs with the
  defaults. The README shows how a drop-in changes the command line.

### Makefile

- New variable `USERUNITDIR`, default `$(PREFIX)/lib/systemd/user`, with the same `~`
  expansion as `PREFIX`.
- `install`, on Linux only (`uname -s` is `Linux`): writes the unit to
  `$(DESTDIR)$(USERUNITDIR)/sagiri-session.service` with mode 644, with `@BINDIR@` replaced
  by `$(PREFIX)/bin` (the path at run time, without `DESTDIR`). The rendered file is
  written straight to its destination; nothing is created in the checkout.
- `install` still does not build, and still installs nothing if a binary is missing.
- `uninstall` removes the unit file too.
- `help` lists `USERUNITDIR` and says that `install` also installs the unit on Linux.

### `packaging/arch/PKGBUILD`

- `pkgrel=2`: the package content changes while the version stays.
- No other change is expected: `make install DESTDIR="$pkgdir" PREFIX=/usr` puts the unit
  in `/usr/lib/systemd/user/`.

### Documentation

`README.md`:

- "Session mode": a subsection "Starting with systemd".
  - `systemctl --user enable --now sagiri-session.service`, and `disable --now` to undo it.
  - The unit is started by `graphical-session.target`. A window manager session that never
    reaches that target (plain i3 started from a display manager, for example) starts the
    unit from its config instead:
    `exec --no-startup-id systemctl --user start sagiri-session.service`.
  - The user manager needs `DISPLAY` and `XAUTHORITY` in its environment. Most
    distributions import them at X start-up; where they are missing,
    `dbus-update-activation-environment --systemd DISPLAY XAUTHORITY` in the session
    start-up imports them.
  - The unit replaces the line `exec --no-startup-id sagiri-session run`. With both in
    place the second process finds the first one running and exits with an error.
  - `systemctl --user restart sagiri-session.service` after a change of the config file or
    of the keyboard layout.
  - Starting a session at login: a drop-in created with
    `systemctl --user edit sagiri-session.service` that clears `ExecStart=` and sets it
    again with `--active`.
  - `journalctl --user -u sagiri-session.service` shows the messages of the process. They
    never contain copied text.
- "Installing": `make install` and the Arch package install the unit on Linux. An install
  into the home directory needs `USERUNITDIR=~/.local/share/systemd/user`, because
  `~/.local/lib/systemd/user` is not on the search path of the user manager.
- The existing i3 lines stay as the alternative without systemd.

## Out of scope

- Socket activation, and starting the session process from `sagiri-session toggle` when
  none is running (backlog).
- A separate exit code for configuration errors (backlog). See the design notes.
- A system unit, a launchd agent for macOS, session mode on Wayland.
- Enabling the unit from `make install` or from the package. Enabling is the user's step.
- Any change to the Rust code.

## Design notes

- **A user unit, not a system unit.** Session mode works on one user's X display and
  reads that user's key. It belongs to the user manager.
- **`graphical-session.target` on all three of `PartOf`, `After` and `WantedBy`.** The
  process starts when the graphical session is up and stops with it. This is the pattern
  described in `systemd.special(7)`.
- **The two conditions prevent restart loops.** `sagiri-session run` exits with status 1
  under Wayland and without `DISPLAY`. With `Restart=on-failure` that would be a loop on a
  TTY or SSH login and in a Wayland session. A failed condition skips the start and is not
  a failure. `ConditionEnvironment` looks at the environment of the user manager, which is
  where the service gets its environment from.
- **A wrong `hotkey` and a combination that is already grabbed also exit with status 1**
  and are restarted. The start rate limit of systemd (by default five starts in ten
  seconds) ends that loop and leaves the unit in the state `failed`, with the reason in the
  journal. Telling these errors apart needs another exit code and is not part of this
  ticket.
- **A stop is clean.** systemd sends SIGTERM, which `sagiri-session` turns into a normal
  shutdown with status 0 and a removed socket.
- **`XDG_RUNTIME_DIR` is set** in every user service, so the default control socket path
  works and `sagiri-session toggle` from a key binding reaches the process.
- **Only `NoNewPrivileges`.** Most sandboxing settings need privileges that a user manager
  does not have and then fail at start. The process needs the X socket, the key file, the
  runtime directory and `notify-send`, so there is little to take away.
- **A template with `@BINDIR@`** because `ExecStart=` needs an absolute path and the
  prefix is only known at install time.
- **Verify against the current systemd documentation** (`systemd.unit(5)`,
  `systemd.service(5)`, `systemd.special(7)`): the spelling and negation of
  `ConditionEnvironment`, the unit search path of the user manager for each prefix, and
  whether `After=graphical-session.target` together with `WantedBy=` behaves as described.
  Where the documentation differs from this ticket, follow the documentation and say so in
  the report.

## Acceptance criteria

Each is a command; the report shows the command and its result.

- [ ] `make release` followed by `make install DESTDIR="$tmp" PREFIX=/usr` creates
      `$tmp/usr/lib/systemd/user/sagiri-session.service` with mode 644 next to the three
      binaries, and nothing else.
- [ ] The installed unit contains `ExecStart=/usr/bin/sagiri-session run` and no `@`.
- [ ] `make install DESTDIR="$tmp" PREFIX=/usr USERUNITDIR=/usr/share/systemd/user` puts
      the unit in that directory instead.
- [ ] `make uninstall` with the same variables leaves no file behind.
- [ ] After each of these commands `git status --porcelain` is empty.
- [ ] `systemd-analyze --user verify` reports no problem for a unit rendered with
      `@BINDIR@` pointing at a directory that holds the built `sagiri-session`.
- [ ] `make pkg` succeeds, and `pacman -Qlp <package>` lists the five files of ticket 0029
      plus `/usr/lib/systemd/user/sagiri-session.service`; `pacman -Qip <package>` shows
      the release `2`.
- [ ] `make help` lists `USERUNITDIR`.
- [ ] `README.md` has the "Starting with systemd" subsection with every point of the
      scope.
- [ ] `scripts/check.sh` passes.

Manual check, the owner's step (listed in the report as not done by the implementer):

- [ ] With the package installed: `systemctl --user enable --now sagiri-session.service`
      starts the process, `sagiri-session status` prints `inactive`,
      `sagiri-session toggle` switches it, and the journal shows the start and stop
      notices and no copied text.
- [ ] `systemctl --user stop sagiri-session.service` ends the process with status 0 and
      the socket file is gone.
- [ ] After `pkill -KILL sagiri-session` the process is started again.
- [ ] After a new login the process runs without a manual start.

## Test plan

- The commands of the acceptance criteria on a Linux machine with systemd; the package
  criteria need `makepkg`. On a system without them the implementer says which criteria
  could not be run.
- On macOS, `make install DESTDIR="$tmp"` installs the three binaries and no unit. If no
  macOS machine is at hand, the report says that this was not run.
