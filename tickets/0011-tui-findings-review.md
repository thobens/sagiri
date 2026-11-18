---
id: "0011"
title: "TUI: findings review"
status: done
depends_on: ["0010"]
---

# 0011 — TUI: findings review

## Why

Detection is heuristic. Before copying, the user needs to see what Sagiri replaced, spot
what it got wrong, and switch a false positive off without leaving the tool. This is the
main advantage of a terminal UI over a plain filter.

## Scope

In `crates/sagiri/src/tui/`.

### Highlighting

- Every finding is highlighted in the `Original` pane (its original text) and in the
  `Obfuscated` pane (its replacement), using `Finding::span` and `Rendered::spans`.
- Each `Kind` has one fixed colour from the 16-colour ANSI palette. Replaced findings are
  shown bold in that colour.
- A finding that is switched off is shown dim and underlined in both panes.

### Findings panel

- A panel titled `Findings` below the text panes. Height: the number of entries plus the
  border, at most one third of the screen. Hidden when there are no findings.
- One entry per distinct (kind, original text), in order of first appearance:

  ```
  [x] IPv4       172.55.0.1 → 126.143.4.12   ×3
  [ ] Hostname   main.acme.com → kuze.poba.com   ×1
  ```

  `[x]` means replaced, `[ ]` means kept. The kind label is `Kind::label()`, in the kind's
  colour. Values
  that do not fit are shortened in the middle with `…`.
- The status line shows `N replaced · M kept`.

### Interaction

| Key | Where | Action |
|---|---|---|
| `Tab` | anywhere | switch focus between the text panes and the findings panel |
| `↑` `↓` `k` `j` | findings panel | move the selection |
| `Space` | findings panel | switch the selected entry on or off |
| `t` | findings panel | switch every entry of the selected entry's kind on or off |
| `a` | anywhere | switch everything back on |

- Moving the selection scrolls the text panes so that the first occurrence of the selected
  entry is visible.
- Switching an entry re-renders the output with `render` and its `include` filter. It does
  not run `analyze` again.
- **Copy copies what is shown:** entries that are switched off keep their original text in
  the copied output.
- The focused area has a highlighted border. All keys from ticket 0010 keep working; with
  the findings panel focused, `↑` `↓` `k` `j` move the selection instead of scrolling.

### Session memory

- Switched-off entries (kind + original text) and switched-off kinds are remembered until
  Sagiri exits, across new pastes and across `x` (clear). A later paste containing the same
  value shows it as kept.
- `a` forgets them all.
- They are held in memory only and never written to disk.

## Out of scope

- Persisting decisions. A permanent exception belongs in the allowlist (ticket 0012).
- Adding a finding by hand (marking text Sagiri missed).
- Editing a replacement.
- Filtering or searching the findings list.

## Design notes

- **Why decisions are not saved.** Saving "do not replace this value" means writing an
  original value to disk, which contradicts the product decision that Sagiri stores no
  originals. The config allowlist is the deliberate, user-written exception.
- **Why per (kind, original).** A value occurring ten times is one decision, not ten.
- **Colour is not the only signal:** the `[x]`/`[ ]` marker, bold and underline carry the
  same information for users who cannot rely on colour.
- The two-phase engine API (`analyze` once, `render` per toggle) exists for this ticket;
  toggling must feel instant even on a large paste.

## Acceptance criteria

`App` tests:

- [ ] `entries_group_by_kind_and_original`: a text with one address three times and another
      once gives two entries with counts 3 and 1, in order of first appearance.
- [ ] `toggle_entry_keeps_original_in_output`: after switching an entry off, the output
      contains the original at every occurrence; other findings are still replaced.
- [ ] `toggle_does_not_reanalyze`: toggling does not call `Engine::analyze` (count the
      calls through a wrapper or a counter in a test rule).
- [ ] `toggle_kind_affects_all_entries_of_that_kind`.
- [ ] `enable_all_restores_everything`.
- [ ] `copy_respects_toggles`: the fake clipboard receives the output with kept originals.
- [ ] `decisions_survive_new_paste_and_clear`: an entry switched off stays off when the
      same value is pasted again, also after `x`.
- [ ] `selection_scrolls_to_first_occurrence`: selecting an entry whose first occurrence is
      on line 500 changes the vertical offset so that line is visible.
- [ ] `status_counts`: `N replaced · M kept` matches the entries' state.
- [ ] `focus_changes_arrow_key_meaning`: with the findings panel focused, `↓` moves the
      selection and the text does not scroll.

Rendering tests with `TestBackend`:

- [ ] Cells of a finding in the `Original` pane and of its replacement in the `Obfuscated`
      pane carry the kind's colour and bold.
- [ ] Cells of a switched-off finding are dim and underlined in both panes.
- [ ] The findings panel shows `[x]`/`[ ]`, the kind label, `original → replacement` and
      the count; it is absent when there are no findings.
- [ ] A long value is shortened with `…` and the row does not overflow the panel.
- [ ] Every `Kind` has a colour and no two kinds share one.

Manual check in a real terminal (report as done or not possible):

- [ ] Paste a text with several findings, switch one off, copy, and confirm the pasted
      result keeps that original and replaces the rest.

And:

- [ ] `scripts/check.sh` passes.

## Test plan

- Unit tests in `tui/app.rs` and `tui/ui.rs` as named above.
- The manual check.
