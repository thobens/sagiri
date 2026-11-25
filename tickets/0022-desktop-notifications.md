---
id: "0022"
title: Desktop notifications
status: done
depends_on: ["0020"]
---

# 0022 — Desktop notifications

## Why

A session changes what the user pastes, and nothing on screen shows whether one is active.
The dangerous mistake is to believe that a session is on when it is not, and to paste real
data. A desktop notification on every start, stop and failure makes the state visible
without a window of its own.

## Scope

In `crates/sagiri/src/session/notify.rs` and the wiring of `sagiri-session run`.

### `CommandNotifier`

- Runs `notify-send` with fixed arguments for each `Notice`.
- `pub fn argv(notice: Notice) -> Vec<&'static str>`, a pure function:

  | Notice | Urgency | Summary | Body |
  |---|---|---|---|
  | `Started` | normal | `Sagiri session started` | `Copied text is now obfuscated.` |
  | `Stopped` | normal | `Sagiri session stopped` | `Copied text is no longer changed.` |
  | `ClipboardCleared` | critical | `Sagiri could not replace the clipboard` | `The clipboard was cleared instead.` |
  | `OriginalStillOnClipboard` | critical | `Sagiri could not replace the clipboard` | `The clipboard still holds the original text.` |
  | `PrimaryNotUpdated` | normal | `Sagiri could not update the primary selection` | `A middle click may paste other text than the clipboard holds.` |
  | `BackendLost` | critical | `Sagiri session ended` | `The connection to the display was lost.` |

  The arguments are `--app-name=Sagiri`, `--urgency=<normal|critical>`, the summary and the
  body, in that order.
- `CommandNotifier::new()` uses the program `notify-send`;
  `CommandNotifier::with_program(name)` exists for tests.
- `notify` returns at once. The program runs with stdin, stdout and stderr discarded, and a
  short-lived thread waits for it, so that no zombie process stays behind and a slow or
  hanging notification service never delays the session.
- If the program cannot be started, the first failure writes one line to stderr:
  `sagiri-session: notify-send is not available; desktop notifications are off`. Later
  failures are silent. The session keeps running.

### Wiring

- `sagiri-session run` uses `StderrNotifier` always, and `CommandNotifier` in addition when
  `session.notify` is `true` (the default). A small `Notifiers(Vec<Box<dyn Notifier>>)`
  forwards each notice to all of them.

### Documentation

- `README.md`, "Session mode": notifications are shown through `notify-send` (package
  `libnotify` on Arch Linux), `notify = false` in `[session]` switches them off, and the
  messages always go to standard error as well.

## Out of scope

- Talking to the notification service over D-Bus from Rust.
- A tray icon, sounds, a status-bar block.
- Replacing an earlier notification instead of adding a new one.
- Notifications on macOS.

## Design notes

- **A subprocess instead of a D-Bus crate.** The D-Bus client crates bring an async
  runtime and dozens of dependencies. `notify-send` is on nearly every Linux desktop and is
  optional here.
- **No content can leak by construction.** `Notice` has no text payload, and `argv`
  returns only string literals. No notification ever contains clipboard text, a finding or
  a count of findings.
- **Critical urgency for failures**, because most notification daemons keep critical
  notifications on screen until they are dismissed.
- **Testing without a notification daemon.** Tests never run `notify-send`. They check
  `argv`, and they exercise the spawning code with other programs: a name that does not
  exist, and a short shell script from a temporary directory.

## Acceptance criteria

Unit tests in `notify.rs`:

- [ ] `argv_for_each_notice`: a table over all six notices with the exact arguments.
- [ ] `failures_use_critical_urgency`: exactly the three notices marked critical above.
- [ ] `argv_is_static`: the function's return type holds `&'static str` only (this is the
      compile-time proof that no content is passed; the test names it).
- [ ] `missing_program_warns_once_and_continues`: with a program name that does not exist,
      two notices give a warning count of 1 and no panic.
- [ ] `notifier_does_not_block`: with a script that sleeps two seconds, `notify` returns in
      under 200 ms.
- [ ] `notifiers_forward_to_all`: two `FakeNotifier`s behind `Notifiers` both record the
      notice.
- [ ] `notify_false_uses_stderr_only`: the function that builds the notifier list from
      `SessionConfig` returns one notifier for `notify = false` and two for `true`.

And:

- [ ] `README.md` documents notifications as described.
- [ ] `scripts/check.sh` passes.

Manual checks on a desktop with a notification daemon (listed in the report as done or not
possible):

- [ ] Starting and stopping a session shows the two normal notifications.
- [ ] With `notify = false`, no notification appears and the lines on standard error
      still do.
- [ ] With `notify-send` renamed or removed from `PATH`, `sagiri-session run` prints the
      warning once and the session still works.

## Test plan

- Unit tests in `notify.rs` as named above.
- The manual checklist. An implementer without a desktop session states that in the
  report.
