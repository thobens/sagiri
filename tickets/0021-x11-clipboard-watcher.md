---
id: "0021"
title: X11 clipboard watcher
status: todo
depends_on: ["0020"]
---

# 0021 — X11 clipboard watcher

## Why

`sagiri-session` can be started, stopped and asked for its state, but it does not yet learn
that something was copied. On X11 the XFixes extension reports every change of a selection's
owner. This ticket adds a watcher thread that turns those reports into events. With it,
session mode works end to end: bind a key in the window manager to `sagiri-session toggle`,
copy, paste the obfuscated text.

## Scope

In `crates/sagiri`. Add `x11rb` 0.13 with the feature `xfixes` as a Linux-only dependency
(see `tickets/README.md`, "Crates").

### Backend choice (`src/session/backend.rs`)

- `pub enum Backend { X11, Unsupported(&'static str) }`.
- `pub fn detect(env: EnvLookup<'_>) -> Backend`, a pure function:

  | Platform and environment | Result |
  |---|---|
  | not Linux | `Unsupported("session mode needs Linux with X11")` |
  | `WAYLAND_DISPLAY` set and not empty | `Unsupported("Wayland sessions are not supported yet")` |
  | `DISPLAY` unset or empty | `Unsupported("no X display: DISPLAY is not set")` |
  | otherwise | `X11` |

- `backend::start` calls `detect`. `Unsupported(reason)` becomes the error
  `session mode is not available: <reason>`; `X11` starts the watcher.

### Watcher (`src/session/x11.rs`, `#[cfg(target_os = "linux")]`)

- `pub fn start(events: Sender<Event>, settings: &SessionConfig) -> anyhow::Result<()>`:
  1. connects to the X server; a failure is the error `cannot connect to the X server`;
  2. negotiates the XFixes version; a missing extension is the error
     `the X server has no XFixes extension`;
  3. asks for selection-owner notifications for `CLIPBOARD` on the root window;
  4. starts a thread that blocks in `wait_for_event` and returns.
- Steps 1 to 3 run on the calling thread, so that their errors end `sagiri-session run`
  with exit code 1 before a session can start.
- The thread sends `Event::Changed(Selection::Clipboard)` for each owner-change
  notification of `CLIPBOARD`. It ignores everything else.
- When the connection fails or closes, it sends `Event::BackendLost` and ends.
- `fn selection_for(atom: u32, clipboard: u32, primary: u32) -> Option<Selection>`, a pure
  function used by the thread.

### Binary

- `sagiri-session run` now works on X11. The message of ticket 0020 for "no backend" is
  replaced by the ones above.

### Documentation

`README.md`, new section "Session mode":

- What it does, in two sentences.
- How to run it: `sagiri-session run` at login, `sagiri-session toggle` on a key. With the
  i3 lines:

  ```
  exec --no-startup-id sagiri-session run
  bindsym $mod+Shift+o exec --no-startup-id sagiri-session toggle
  ```

- The commands `start`, `stop`, `toggle`, `status`, `quit` and what they print.
- The rules and limits, in plain words:
  - Copied text comes back as plain text; formatting is lost.
  - Copied files and images are not changed. File names are not obfuscated.
  - For a short moment after each copy the original is on the clipboard. A clipboard
    manager with a history may record it; its history should be paused or switched off
    while a session is used. Session mode prevents accidental pasting and does not protect
    against other programs on the same machine.
  - Copying a part of an already obfuscated text obfuscates that part again.
  - Linux with X11 only.
- Where messages appear until desktop notifications exist (ticket 0022): on the standard
  error of `sagiri-session run`.

## Out of scope

- The PRIMARY selection (tickets 0024 and 0025).
- The hotkey (ticket 0023).
- Desktop notifications (ticket 0022).
- Wayland and macOS backends (backlog).
- Automated tests against a real X server (ticket 0026).

## Design notes

- **XFixes instead of polling.** Polling cannot see a second copy of the same text, keeps
  the original on the clipboard for up to one interval, and costs a connection per read.
  XFixes delivers one event per copy and lets the thread sleep in between.
- **`x11rb` directly.** It is already in `Cargo.lock` through `arboard`, forbids unsafe
  code, and the code needed is small. No clipboard-watching crate is added.
- **Two connections.** The watcher has its own connection; `arboard` has another, on the
  main thread. The watcher never reads or writes a selection.
- **Sagiri's own writes arrive as events too.** `Session` recognizes them (ticket 0019).
  The watcher does not filter.
- **Only "new owner" notifications are requested.** A selection whose owner went away
  holds nothing to obfuscate.
- **Read the `x11rb` 0.13 documentation** for the exact calls: `xfixes_query_version`
  (it must precede every other XFixes request), `xfixes_select_selection_input` with the
  set-selection-owner mask, `intern_atom` for `CLIPBOARD`, and the event variant
  `XfixesSelectionNotify` with its `selection` field.
- **Refusing Wayland.** Under Wayland, X11 clients see the clipboard only partly. A
  session that silently misses copies is worse than none.
- **macOS** has no `x11.rs`; `detect` answers before anything X11-related is needed.

## Acceptance criteria

Unit tests in `backend.rs` (injected environment; the Linux rows under
`#[cfg(target_os = "linux")]`):

- [ ] `detect_x11_when_display_set`.
- [ ] `detect_refuses_wayland_session`: also when `DISPLAY` is set as well.
- [ ] `detect_refuses_without_display`: unset and empty.
- [ ] Not Linux: `detect_is_unsupported`.

Unit test in `x11.rs`:

- [ ] `selection_event_mapping`: the clipboard atom, the primary atom and an unrelated
      atom.

Integration tests in `crates/sagiri/tests/session.rs`:

- [ ] Linux: `run_without_display_exits_1`: `run` without `DISPLAY` and `WAYLAND_DISPLAY`
      exits 1 with the "no X display" reason and leaves no socket file. It replaces
      `run_without_backend_exits_1`.
- [ ] Linux: `run_under_wayland_exits_1`: with `WAYLAND_DISPLAY=wayland-0` in the child's
      environment.
- [ ] Not Linux: `run_is_unsupported`.

And:

- [ ] `cargo tree -p sagiri -i x11rb` shows one version.
- [ ] `grep -rn "allow-unsafe-code" crates/` prints nothing.
- [ ] `README.md` has the "Session mode" section with every point listed above.
- [ ] `scripts/check.sh` passes, on Linux and on macOS (CI).

Manual checks on X11 (listed in the report as done or not possible):

- [ ] `sagiri-session run` in one terminal; `sagiri-session toggle` in another prints
      `active`.
- [ ] Copy `server 172.55.0.1 is down` in a terminal and in a browser; pasting gives the
      same obfuscated text both times.
- [ ] Copy an image in a browser and a file in a file manager; both paste unchanged.
- [ ] While nothing is copied, `sagiri-session` uses no CPU (`top`).
- [ ] After `sagiri-session stop`, copied text pastes unchanged.
- [ ] `sagiri-session quit` ends the process with exit code 0.

## Test plan

- Unit tests for `detect` and `selection_for`; integration tests for the refusal paths.
- The manual checklist is the only proof of the X11 code in this ticket; ticket 0026
  automates it. An implementer without a display states that in the report.
