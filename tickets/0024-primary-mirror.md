---
id: "0024"
title: PRIMARY mirror
status: todo
depends_on: ["0020"]
---

# 0024 — PRIMARY mirror

## Why

On X11 there are two selections. `CLIPBOARD` is what copy and paste use. `PRIMARY` holds
the text that is currently selected with the mouse, and a middle click (the mouse wheel
button) pastes it. During a session the clipboard is obfuscated, but a middle click still
pastes the original, because the selected text is untouched. With `primary = "mirror"`,
every text that the session writes to the clipboard is written to `PRIMARY` as well. After
a copy, both ways of pasting give the obfuscated text.

## Scope

In `crates/sagiri/src/session/core.rs` and the wiring of `sagiri-session run`.

- `Session::with_primary(self, mode: PrimaryMode) -> Self` (pinned in `tickets/README.md`).
  The default is `PrimaryMode::Off`.
- With `PrimaryMode::Mirror`, when `on_change(Selection::Clipboard)` ends in
  `Outcome::Replaced`:
  - the same obfuscated text is written to `Selection::Primary`;
  - on success it is remembered as the last text written to `PRIMARY`;
  - on failure `Notice::PrimaryNotUpdated` is sent. The outcome stays `Replaced`; the
    clipboard is not cleared.
- Nothing is mirrored for the outcomes `Own`, `Skipped`, `Failed` and `Inactive`.
- `PrimaryMode::Off` and `PrimaryMode::Watch` never write to `PRIMARY` in this ticket.
  (`Watch` gets its behaviour in ticket 0025.)
- `sagiri-session run` passes `config.session.primary` to the session.

### Documentation

- `README.md`, "Session mode" and "Configuration": what `primary = "mirror"` does, and
  these two limits:
  - Text that is selected but not copied is still pasted unchanged by a middle click.
  - After a copy, the application that the text was copied from loses its visible
    selection, because `PRIMARY` now belongs to Sagiri.

## Out of scope

- Reacting to changes of `PRIMARY` itself (ticket 0025).
- Any X11 code: the mirror is a second `write_text` through `SelectionClipboard`.
- Wayland and macOS.

## Design notes

- **Mirror is the clean half of middle-click support.** It acts only after an explicit
  copy, so it never interferes with selecting text. That is why it is separate from the
  experimental watch mode.
- **A failed mirror is not a failed replacement.** The clipboard already holds the
  obfuscated text. Clearing it would throw away a correct result. The user is told that a
  middle click may paste something else.
- **Remembering the mirrored text** matters for ticket 0025: when `PRIMARY` is watched,
  Sagiri's own mirror write must be recognized as its own.
- The session never reads `PRIMARY` in mirror mode.

## Acceptance criteria

Unit tests in `core.rs`, with `FakeSelections` and `FakeNotifier`:

- [ ] `mirror_writes_primary_after_replacement`: both selections hold the same obfuscated
      text.
- [ ] `off_never_touches_primary`: read, write and clear counters of the primary
      selection stay 0.
- [ ] `watch_mode_does_not_mirror`.
- [ ] `mirror_skips_own_and_skipped_outcomes`: no primary write for a repeated event and
      for a file copy.
- [ ] `mirror_failure_keeps_clipboard_replacement`: the clipboard holds the obfuscated
      text, its clear counter is 0, the outcome is `Replaced`, and the notice is
      `PrimaryNotUpdated`.
- [ ] `mirrored_text_counts_as_own_write`: after a mirror,
      `on_change(Selection::Primary)` gives `Own`.
- [ ] `activation_mirrors_too`: text on the clipboard when the session starts ends up on
      both selections.

And:

- [ ] `README.md` documents mirror mode with both limits.
- [ ] `scripts/check.sh` passes.

Manual checks on X11 with `primary = "mirror"` (listed in the report as done or not
possible):

- [ ] Copy `server 172.55.0.1 is down`, then middle-click into a terminal: the obfuscated
      text is pasted, the same as with the paste shortcut.
- [ ] Select other text without copying and middle-click: that text is pasted unchanged.
- [ ] With `primary = "off"`, a middle click after a copy pastes the original selection.

## Test plan

- Unit tests in `core.rs` as named above.
- The manual checklist. An implementer without a display states that in the report.
