---
id: "0023"
title: X11 global hotkey
status: done
depends_on: ["0021"]
---

# 0023 — X11 global hotkey

## Why

Until now a session is toggled by a command that the user binds to a key in the window
manager. That works, but the shortcut then lives in another program's configuration. With
this ticket `sagiri-session` grabs a key combination itself, taken from `session.hotkey` in
Sagiri's config file. The `toggle` command stays; both ways can be used side by side.

## Scope

In `crates/sagiri`.

### Syntax (`src/session/hotkey.rs`)

- `pub fn parse(text: &str) -> Result<Hotkey, HotkeyError>`.
- Parts are separated by `+`. Spaces around a part are ignored. Case is ignored.
- Modifiers: `ctrl` (also `control`), `alt`, `shift`, `super` (also `win`).
- Exactly one key: a letter `a` to `z`, a digit `0` to `9`, `f1` to `f12`, `insert` or
  `pause`.
- At least one of `ctrl`, `alt` or `super` is required.
- Errors, as fixed texts that never repeat the input:

  | Input | `HotkeyError` text |
  |---|---|
  | empty, or an empty part (`ctrl++o`) | `is empty or has an empty part` |
  | no key | `names no key` |
  | two keys | `names more than one key` |
  | an unknown word | `contains an unknown modifier or key` |
  | the same modifier twice | `repeats a modifier` |
  | only `shift`, or no modifier | `needs ctrl, alt or super` |

- `Hotkey` holds the four modifier flags and the X11 keysym of the key.

### Pure helpers (same file)

- `keycodes_for(keysym, min_keycode, keysyms_per_keycode, keysyms) -> Vec<u8>`: the
  keycodes whose first column in the server's keyboard mapping is the keysym.
- `grab_masks(hotkey) -> [u16; 4]`: the modifier mask alone, with Caps Lock, with Num
  Lock, and with both.
- `matches(hotkey, keycodes, event_keycode, event_state) -> bool`: compares the keycode
  and the state restricted to Shift, Control, Mod1 and Mod4.
- `Debounce`: `accept(time_ms: u32) -> bool` is `false` for a press less than 300 ms after
  the last accepted one. It works across the wrap-around of the server's 32-bit time.

### Grab (`src/session/x11.rs`)

- If `session.hotkey` is set, `x11::start` grabs it on the root window on the watcher's
  connection, once per keycode and mask, before the thread starts.
- Start-up errors, all with exit code 1:

  | Situation | Message |
  |---|---|
  | the syntax is wrong | `session.hotkey <HotkeyError text>` |
  | the key is not on the keyboard layout | `session.hotkey names a key that the keyboard layout does not have` |
  | another program holds the combination | `session.hotkey is already taken by another program` |

- The watcher thread sends `Event::Toggle` for each key press that `matches` and that
  `Debounce` accepts.
- Without `session.hotkey` nothing is grabbed.

### Binary

- `sagiri-session run` parses the hotkey right after loading the config, before the backend
  is chosen. A wrong hotkey is therefore reported on every platform and without a display.

### Documentation

- `README.md`, "Session mode" and "Configuration": the syntax, an example, the two notes
  below, and that the `toggle` command keeps working.
  - A combination that the window manager already uses cannot be grabbed; `sagiri-session
    run` then exits with a message.
  - After a change of the keyboard layout, `sagiri-session` has to be restarted.

## Out of scope

- Wayland and macOS.
- Following keyboard layout changes while running (backlog).
- More than one hotkey; separate keys for start and stop.
- Keys that need Shift or AltGr on the current layout.

## Design notes

- **The grab uses the watcher's connection**, so one thread receives selection
  notifications and key presses. No second thread, no second crate.
- **Four grabs per keycode.** X11 treats Caps Lock and Num Lock as modifiers. A grab for
  `ctrl+alt+o` alone does not fire while Num Lock is on. Num Lock is assumed to be Mod2,
  which holds on every common setup; a different mapping is a known limitation.
- **Refusing instead of guessing.** If the grab fails, the user must learn it at start-up,
  not when the shortcut silently does nothing. Use the checked form of the request so that
  the X error (`Access`) reaches the caller.
- **Auto-repeat.** Holding the combination sends repeated presses. Without `Debounce` the
  session would flicker on and off.
- **Keysyms.** Letters and digits are their ASCII code points (letters in lower case).
  `F1` to `F12` are `0xffbe` to `0xffc9`, `Insert` is `0xff63`, `Pause` is `0xff13`.
  Check these against `keysymdef.h` or the `x11rb` documentation.
- **Read the `x11rb` 0.13 documentation** for `grab_key`, `get_keyboard_mapping` (the
  reply's `keysyms_per_keycode` and flat `keysyms` list; `min_keycode` and `max_keycode`
  come from the connection set-up) and the `KeyPress` event's `detail`, `state` and `time`
  fields.
- Error texts never repeat the configured value. They follow the config rule of ticket
  0012.

## Acceptance criteria

Unit tests in `hotkey.rs`:

- [ ] `parses_ctrl_alt_o`.
- [ ] `parsing_ignores_case_and_spaces`: `Ctrl + Alt + O`.
- [ ] `aliases`: `control` and `win`.
- [ ] `parses_function_and_named_keys`: `super+f12`, `ctrl+insert`, `alt+pause`,
      `ctrl+7`.
- [ ] `rejects_each_error_case`: a table with one row per line of the error table, each
      with the exact text.
- [ ] `error_text_never_contains_input`.
- [ ] `keycodes_for_keysym`: a small made-up mapping with the keysym in the first column
      of two keycodes and in the second column of a third; two keycodes are returned.
- [ ] `grab_masks_cover_lock_variants`.
- [ ] `match_ignores_numlock_and_capslock`.
- [ ] `match_requires_exact_modifiers`: an extra Shift does not match.
- [ ] `repeat_within_300ms_is_ignored`, and `debounce_survives_time_wraparound`.

Integration tests in `crates/sagiri/tests/session.rs`:

- [ ] `invalid_hotkey_exits_1`: a config with `hotkey = "o"` and no display in the child's
      environment gives the message `sagiri-session: session.hotkey needs ctrl, alt or
      super`.
- [ ] `valid_hotkey_reaches_backend_check`: with `hotkey = "ctrl+alt+o"` and no display,
      the message is the one about the display.

And:

- [ ] `README.md` documents the hotkey as described.
- [ ] `scripts/check.sh` passes, on Linux and on macOS (CI).

Manual checks on X11 (listed in the report as done or not possible):

- [ ] With `hotkey = "ctrl+alt+o"`, the combination toggles the session; `sagiri-session
      status` confirms each change.
- [ ] It works with Num Lock on, with Num Lock off and with Caps Lock on.
- [ ] Holding the combination for two seconds toggles once.
- [ ] With the same combination bound in the window manager, `sagiri-session run` exits 1
      with the "already taken" message.
- [ ] `sagiri-session toggle` still works while the hotkey is set.

## Test plan

- Unit tests in `hotkey.rs`; two integration tests for the start-up order.
- The manual checklist; ticket 0026 automates the first item. An implementer without a
  display states that in the report.
