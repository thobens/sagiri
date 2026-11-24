---
id: "0019"
title: Session core
status: done
depends_on: ["0015"]
---

# 0019 — Session core

## Why

Session mode is: while a session is active, every text that lands on the clipboard is
replaced by its obfuscated form. The rules behind that sentence (what counts as text, what
happens on failure, how Sagiri avoids processing its own output) are the part that must be
right, and none of them depends on X11. This ticket implements them as a state machine that
is driven by events and tested with fakes. Sockets, signals and the display follow in
tickets 0020 and 0021.

## Scope

In `crates/sagiri/src/session/`. The names are pinned in `tickets/README.md`, section
"Binary crate API".

- `mod.rs`: `pub mod core; pub mod notify;`, exported from `lib.rs` as `pub mod session`.
- `notify.rs`: `Notice`, the `Notifier` trait, and a test-only `FakeNotifier` that records
  the notices it receives (clones share state).
- `core.rs`: `Command`, `Event`, `Outcome`, `Exit`, `Session`, `run_loop`.

### `Session`

A session is inactive after `Session::new`.

`on_change(selection)`:

1. Inactive: return `Outcome::Inactive`. The clipboard is not touched in any way.
2. `read(selection)` fails: go to "Failure" below.
3. `Content::Files` or `Content::Other`: return `Outcome::Skipped`. Nothing is written.
4. `Content::Text(t)`:
   - `t` equals the last text this session wrote to that selection: return `Outcome::Own`.
   - Otherwise write `engine.obfuscate(t)` with `write_text`, remember it as the last text
     written to that selection, and return `Outcome::Replaced`. The write happens even
     when obfuscation changed nothing.
   - The write fails: go to "Failure".

Failure:

- `clear(selection)`. If that works, send `Notice::ClipboardCleared`; if it fails too, send
  `Notice::OriginalStillOnClipboard`. Return `Outcome::Failed`.

`set_active(active)`:

- No change of state: nothing happens, no notice.
- Inactive to active: send `Notice::Started`, then run `on_change(Selection::Clipboard)`.
- Active to inactive: send `Notice::Stopped`.
- The remembered last-written texts are kept for the life of the `Session`, across stop
  and start.

### `run_loop`

Receives events until one ends the loop.

| Event | Effect |
|---|---|
| `Changed(selection)` | `on_change(selection)` |
| `Command { Start }` | `set_active(true)` |
| `Command { Stop }` | `set_active(false)` |
| `Command { Toggle }` | flip |
| `Command { Status }` | nothing |
| `Command { Quit }` | end with `Exit::Quit` |
| `Toggle` | flip |
| `BackendLost` | end with `Exit::BackendLost` |
| `Shutdown` | end with `Exit::Shutdown` |
| channel closed | end with `Exit::Shutdown` |

- Every `Command` gets a reply: the state after the command (`true` is active). For `Quit`
  the reply is `false`. A reply that cannot be delivered is ignored.
- Ending while active deactivates first: `Notice::Stopped` for `Quit` and `Shutdown`.
  For `BackendLost` the session becomes inactive and only `Notice::BackendLost` is sent.

## Out of scope

- The control socket, signals, the binary (ticket 0020).
- Detecting clipboard changes (ticket 0021). Here, events come from tests.
- A real notifier (tickets 0020 and 0022).
- Mirroring to or watching the PRIMARY selection (tickets 0024 and 0025). `on_change`
  already works for either selection.
- Keeping rich formats (backlog).

## Design notes

- **The rules come from `tickets/README.md`, "Session mode rules".** They are repeated in
  the Scope above in executable form.
- **Why the write always happens.** A browser offers copied text as plain text and as
  HTML. If only the plain text is checked and nothing is written when it has no finding,
  the HTML stays on the clipboard with whatever it carries. Writing plain text replaces
  every format.
- **Own-write detection by comparison.** After a write Sagiri owns the selection, and the
  watcher reports that change like any other. Obfuscated output looks real, so processing
  it again would change it again. Comparing with the last written text stops that. The
  remembered text is Sagiri's own output, never an original, and exists in memory only.
- **Known limit.** Copying a part of an obfuscated text during a session obfuscates that
  part once more. This is harmless and documented in the user README by ticket 0021.
- **A failed read is treated like a failed write.** If the content cannot be read as text,
  nobody knows whether it is sensitive. Clearing is the safe side.
- **Clearing triggers another change event.** It reads as `Other` and is skipped.
- **Single thread.** `Session` owns the clipboard and is used from one thread only. The
  fakes therefore need no `Send`.
- Nothing in this module prints or logs. No type here derives a `Debug` that could show
  clipboard text.

## Acceptance criteria

Unit tests in `core.rs`, with `FakeSelections`, `FakeNotifier` and an engine built from the
test key. The sample text is `server 172.55.0.1 is down`.

- [ ] `inactive_session_never_touches_clipboard`: read, write and clear counters stay 0.
- [ ] `active_session_replaces_text`: the fake holds `engine.obfuscate(..)`; the outcome
      is `Replaced`.
- [ ] `text_without_findings_is_still_written`: the write counter is 1.
- [ ] `own_write_is_not_reprocessed`: a second `on_change` gives `Own` and no further
      write.
- [ ] `recopied_output_is_not_reprocessed`: the fake is set again to the same obfuscated
      text; the outcome is `Own`.
- [ ] `files_are_left_untouched` and `non_text_is_left_untouched`: outcome `Skipped`, no
      write, no clear.
- [ ] `selections_are_tracked_separately`: a write to the clipboard selection does not
      make the same text on the primary selection count as own.
- [ ] `activation_processes_current_clipboard`: text placed before `set_active(true)` is
      replaced, and `Started` is the first notice.
- [ ] `activating_twice_is_a_no_op`: one `Started`, one write.
- [ ] `last_written_survives_stop_and_start`: stop, then start with the obfuscated text
      still in place, gives no second write.
- [ ] `write_failure_clears_and_notifies`: notice `ClipboardCleared`, outcome `Failed`.
- [ ] `read_failure_clears_and_notifies`.
- [ ] `write_and_clear_failure_notifies_original_kept`: notice
      `OriginalStillOnClipboard`.
- [ ] `commands_reply_with_state`: a table over the five commands and both start states.
- [ ] `toggle_event_flips_state`.
- [ ] `quit_and_shutdown_stop_an_active_session`: the exit value and a final `Stopped`.
- [ ] `backend_lost_notifies_and_exits`: `Exit::BackendLost`, notice `BackendLost`, no
      `Stopped`.
- [ ] `closed_channel_ends_the_loop`.
- [ ] `dropped_reply_receiver_is_ignored`.

And:

- [ ] `scripts/check.sh` passes.

## Test plan

- Unit tests only, named as above. Events are sent through a `std::sync::mpsc` channel
  that the test fills before calling `run_loop`.
