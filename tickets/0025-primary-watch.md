---
id: "0025"
title: PRIMARY watch (experimental)
status: todo
depends_on: ["0021", "0024"]
---

# 0025 — PRIMARY watch (experimental)

## Why

Mirror mode (ticket 0024) covers a middle click after a copy. It does not cover the habit
that the X11 primary selection exists for: select text with the mouse, middle-click
somewhere else, no copy in between. With `primary = "watch"` the session replaces the
selected text itself. This has a visible cost that cannot be avoided, so the mode is opt-in
and marked experimental.

## Scope

In `crates/sagiri`.

### What the user must know (goes into `README.md` word for word in meaning)

- As soon as a selection is replaced, the application loses its visible selection: the
  highlight disappears.
- In terminals and other programs that copy "the current selection", a copy shortcut
  pressed after that copies nothing. With watch mode, select and middle-click; do not
  select and then copy.
- Selections made with the keyboard may be replaced before they are complete.
- The mode is experimental and may change or be removed.

### Settle logic (`src/session/settle.rs`)

Applications take `PRIMARY` when a selection starts, not when it is finished. Reading at
once would cut the selection short. A pure state machine decides when to read:

- `pub struct Settle` with
  - `on_owner_change(now)`: a `PRIMARY` owner-change notification arrived;
  - `poll(now, held) -> bool`: `true` exactly once per pending selection, when it should
    be processed. `held` says whether mouse button 1 or Shift is down.
  - `is_pending() -> bool`.
- Rules, with `now` as a monotonic time passed in by the caller:
  - A selection is pending from its first notification.
  - Activity is a notification or a poll with `held == true`.
  - It fires when 150 ms have passed since the last activity.
  - It fires regardless after 30 s of being pending.
  - After firing, the machine is idle until the next notification.

### Watcher (`src/session/x11.rs`)

- With `PrimaryMode::Watch`, the watcher also asks for owner-change notifications of
  `PRIMARY`.
- While `Settle` is pending, the thread stops blocking: every 50 ms it handles the events
  that have arrived, asks the server for the pointer state (`query_pointer` on the root
  window; button 1 and Shift are in the returned mask), and calls `poll`. When `poll`
  fires, it sends `Event::Changed(Selection::Primary)`.
- While nothing is pending, the thread blocks as before.
- With any other mode, `PRIMARY` is not watched.

### Session (`src/session/core.rs`)

- With `PrimaryMode::Watch`, starting a session processes `PRIMARY` as well as the
  clipboard.
- Everything else already works: `on_change(Selection::Primary)` exists since ticket 0019,
  and own writes are recognized per selection.

## Out of scope

- Keeping the highlight in the source application. X11 offers no way to do that.
- Wayland and macOS.
- Combining `watch` with mirroring: in watch mode nothing is mirrored (ticket 0024).
- Making the delays configurable.

## Design notes

- **Honest labelling.** The cost described above follows from how X11 selections work: a
  selection has one owner, and the previous owner is told that it lost it. The README must
  state the cost before the benefit.
- **Why wait for the button.** During a drag the selection grows. Taking `PRIMARY` in the
  middle of a drag ends the drag in most toolkits.
- **Why 30 seconds.** Someone holding the button while scrolling through a long text must
  not leave an original on `PRIMARY` forever. After the limit the selection is processed
  even though that interrupts the drag.
- **Sagiri's own write to `PRIMARY` produces a notification** like any other. It settles
  and is then recognized as an own write by the session. That costs one read, which
  arboard answers from its own memory.
- **Time is injected.** `Settle` never asks the clock itself, so every rule has a unit
  test without sleeping.
- **Which applications behave how** is not known in advance. The manual checklist records
  it, and the README lists the result.
- Read the `x11rb` 0.13 documentation for `query_pointer` and for handling events without
  blocking (`poll_for_event`).

## Acceptance criteria

Unit tests in `settle.rs`:

- [ ] `idle_never_fires`.
- [ ] `fires_after_quiet_period`: a notification at 0 ms, polls without `held`: no fire at
      100 ms, fire at 150 ms.
- [ ] `waits_while_button_is_held`: polls with `held` up to 2 s, then released: fire
      150 ms after the last held poll.
- [ ] `new_notification_restarts_the_wait`.
- [ ] `fires_after_limit_even_when_held`: at 30 s.
- [ ] `fires_once_per_selection`: a poll after the fire returns `false` until the next
      notification.

Unit tests in `core.rs`:

- [ ] `watch_replaces_primary_text`.
- [ ] `own_primary_write_is_not_reprocessed`.
- [ ] `watch_activation_processes_both_selections`.
- [ ] `watch_failure_clears_primary_only`: a failing write to `PRIMARY` clears `PRIMARY`
      and leaves the clipboard selection untouched.

And:

- [ ] `README.md` has the four statements from "What the user must know" in the "Session
      mode" section, before the description of what the mode is good for.
- [ ] `scripts/check.sh` passes, on Linux and on macOS (CI).

Manual checks on X11 with `primary = "watch"` (listed in the report as done or not
possible):

- [ ] In a terminal emulator, a browser and a text field of a GTK or Qt application:
      select `server 172.55.0.1 is down` with the mouse, middle-click into another window.
      The obfuscated text is pasted.
- [ ] For each of the three, the report says whether the highlight disappeared and whether
      a slow drag over several lines was interrupted.
- [ ] Double-click selection of a word and triple-click selection of a line are replaced.
- [ ] While idle, `sagiri-session` uses no CPU.
- [ ] With `primary = "mirror"` and `primary = "off"`, selecting text changes nothing.

## Test plan

- Unit tests in `settle.rs` and `core.rs` as named above.
- The manual checklist is the only proof of the watcher changes. An implementer without a
  display states that in the report, and the owner runs it before the ticket is merged.
