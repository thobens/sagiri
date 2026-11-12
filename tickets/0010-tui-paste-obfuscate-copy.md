---
id: "0010"
title: "TUI: paste, obfuscate, copy"
status: done
depends_on: ["0005"]
---

# 0010 — TUI: paste, obfuscate, copy

## Why

The terminal UI is Sagiri's primary interface. The everyday loop is: paste a text, see the
obfuscated version next to it, copy that, paste it somewhere else. This ticket delivers that
loop. Reviewing and toggling individual findings follows in ticket 0011.

## Scope

In `crates/sagiri`. Add `ratatui`, `crossterm`, `arboard` (with `wayland-data-control`) and
`unicode-width`.

### Start-up

- In `main.rs`, the terminal branch of `mode()` now starts the TUI with the engine from
  `setup::build_engine`.
- If stdout is not a terminal: `sagiri: interactive mode needs a terminal`, exit 2.

### Structure (`src/tui/`)

- `app.rs`: `App` holds all state and has no terminal or clipboard code in it.
  - `pub fn update(&mut self, action: Action)` is the only way state changes.
  - `pub fn action_for(event: &crossterm::event::Event) -> Option<Action>` maps input to
    actions.
- `clipboard.rs`: `trait Clipboard { fn get_text(&mut self) -> anyhow::Result<String>;
  fn set_text(&mut self, text: &str) -> anyhow::Result<()>; }`, a `SystemClipboard` backed
  by one `arboard::Clipboard` that lives as long as the app, and a fake for tests.
- `ui.rs`: `pub fn draw(app: &App, frame: &mut ratatui::Frame)`. Pure rendering.
- `mod.rs`: `pub fn run(engine: Engine) -> anyhow::Result<()>`. Raw mode, alternate screen,
  bracketed paste, the event loop, and teardown on every exit path including a panic.

### Behaviour

- **Paste** replaces the current text. `\r\n` and lone `\r` become `\n` first (terminals
  deliver pasted line breaks as `\r`). Then `Engine::analyze` and `render` run once.
- **Layout.** Two panes titled `Original` and `Obfuscated`: side by side when the terminal
  is at least 100 columns wide, stacked otherwise. One status line at the bottom: on the
  left the number of findings and lines, or the latest message; on the right the key hints.
  With no text, the panes show `Paste text here (terminal paste or Ctrl+V)`.
- **Scrolling.** Lines are not wrapped. Both panes always show the same lines and the same
  horizontal offset. Offsets are clamped to the text.
- **Keys**

  | Key | Action |
  |---|---|
  | terminal paste (bracketed) | replace the text |
  | `Ctrl+V`, `p` | replace the text with the system clipboard |
  | `y`, `Enter` | copy the obfuscated text to the system clipboard |
  | `x`, `Ctrl+L` | clear |
  | `↑` `↓` `k` `j`, `PgUp` `PgDn`, `g` `G` | vertical scroll |
  | `←` `→` `h` `l` | horizontal scroll |
  | `q`, `Esc`, `Ctrl+C` | quit |

  Any other key is ignored.
- **Copy** puts exactly the obfuscated text on the clipboard and shows
  `Copied N characters`. A clipboard error becomes a status message; the app keeps running.
- **Display only:** a tab is shown as four spaces; any other control character (including
  ESC) is shown as `·`; wide characters are measured with `unicode-width`. The copied text
  is never altered by these display rules.
- **Large pastes.** Only the visible lines are turned into widgets on each draw. A 1 MiB
  paste must stay responsive.

## Out of scope

- Highlighting findings, the findings list, toggling (ticket 0011).
- Editing the text, appending pastes, several buffers.
- Line wrapping, mouse support, themes, a help screen.
- Auto-copy on paste and OSC 52 (backlog).
- The config file (ticket 0012).

## Design notes

- **State and rendering are separated from the terminal** so that nearly everything is
  testable: `App::update` with a fake clipboard, and `ui::draw` against ratatui's
  `TestBackend`.
- **Synchronized scrolling is correct** because the engine guarantees that the output has
  the same number of lines as the input.
- **Privacy.** The pasted text lives in memory only. No log file, no crash dump, no state
  file. The panic hook restores the terminal and prints only the panic message; no panic
  message may include text from the buffer.
- **Linux clipboard.** The clipboard content is served by the process that set it. Keep the
  `arboard::Clipboard` alive for the whole session. Without a clipboard manager, the
  content may disappear when Sagiri exits; mention this in the README.
- **Terminals without bracketed paste** deliver a paste as individual key presses, which
  would trigger key bindings. This is a known limitation; `Ctrl+V` works everywhere.
- `tui-textarea` is not compatible with ratatui 0.30 and is not needed: the panes are
  read-only.
- Read the current ratatui and crossterm docs for terminal setup and teardown helpers
  instead of relying on older examples.

## Acceptance criteria

`App` tests with a fake clipboard and an engine built from the test key:

- [ ] `paste_obfuscates`: after pasting `server 172.55.0.1 is down`, the output equals
      `engine.obfuscate(..)` and does not contain `172.55.0.1`.
- [ ] `paste_replaces_previous_text`.
- [ ] `same_value_in_two_pastes_gets_same_replacement`.
- [ ] `line_breaks_are_normalized`: pasting `a\r\nb\rc` gives three lines and no `\r`.
- [ ] `copy_sets_clipboard_to_output`: the fake clipboard holds exactly the output; the
      status message is `Copied N characters`.
- [ ] `ctrl_v_reads_clipboard`.
- [ ] `clipboard_failure_becomes_status_message`, for both reading and writing.
- [ ] `clear_resets_text_and_scroll`.
- [ ] `scroll_is_clamped`: scrolling past either end stays in range, in both directions.
- [ ] `key_mapping`: a table test from events to actions covering every row of the key
      table, plus an unbound key giving `None`.

Rendering tests with `TestBackend`:

- [ ] At 120×30 the panes are side by side; at 80×30 they are stacked. Both titles are
      present.
- [ ] The empty state shows the paste hint.
- [ ] After a paste, the original text is in the `Original` pane, the replacement is in the
      `Obfuscated` pane, and the status line shows the number of findings.
- [ ] `control_characters_are_not_rendered`: after pasting text containing `\x1b[31m` and a
      tab, no cell of the buffer contains ESC or a tab.
- [ ] Wide characters (for example `日本語`) do not shift the following columns.

Manual checks in a real terminal (to be listed in the report as done or not possible):

- [ ] Paste with the terminal's paste shortcut and with `Ctrl+V`.
- [ ] Copy with `y`, paste into another application: it is the obfuscated text.
- [ ] Resize across the 100-column threshold.
- [ ] After `q`, and after a forced panic, the terminal is back to normal.
- [ ] `echo x | sagiri` still runs filter mode.

And:

- [ ] `scripts/check.sh` passes. No test touches the real system clipboard.

## Test plan

- Unit tests in `tui/app.rs` and `tui/ui.rs` as named above.
- The manual checklist. An implementer without an interactive terminal states that
  clearly in the report so the reviewer or the owner can run it.
