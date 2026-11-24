---
id: "0015"
title: Selection-aware clipboard
status: done
depends_on: ["0014"]
---

# 0015 — Selection-aware clipboard

## Why

The TUI only needs "get text" and "set text". The CLI and session mode need more: the
PRIMARY selection (what a middle click pastes on X11), a way to tell text from files and
images, and a way to clear a selection. This ticket adds that as a second trait next to the
existing one, with a fake for tests.

## Scope

In `crates/sagiri/src/clipboard.rs`. The names are pinned in `tickets/README.md`, section
"Binary crate API".

### Types

- `pub enum Selection { Clipboard, Primary }`, `Copy`, `Eq`, `Debug`.
- `pub enum Content { Text(String), Files, Other }`, `Eq`.
  - Its `Debug` output never contains the text: `Text` is shown with its length in bytes
    only.
- `pub trait SelectionClipboard` with `read`, `write_text` and `clear`, each taking a
  `Selection`.

### Classification

- `pub enum Probe<T> { Found(T), Absent, Failed }`: the result of asking a selection for
  one kind of content. `Absent` means the selection does not offer it.
- `pub fn classify(files: Probe<usize>, text: Probe<String>) -> anyhow::Result<Content>`,
  a pure function:
  1. `files` is `Found(n)` with `n > 0`: `Content::Files`.
  2. Otherwise `text` is `Found(s)` with `s` not empty: `Content::Text(s)`.
  3. Otherwise `text` is `Found("")` or `Absent`: `Content::Other`.
  4. Otherwise (`text` is `Failed`): an error with the fixed message
     `the selection could not be read as text`.
- A failed file probe is treated like an absent one.

### `SystemClipboard`

Implements `SelectionClipboard` in addition to `Clipboard`, on the same lazily opened
`arboard::Clipboard`.

- `read`: asks for the file list, then for text, maps each result to a `Probe`
  (`arboard::Error::ContentNotAvailable` is `Absent`, any other error is `Failed`) and
  returns `classify(..)`.
- `write_text` and `clear` act on the given selection.
- On Linux the selection is chosen with arboard's Linux extension traits (`GetExtLinux`,
  `SetExtLinux`, `ClearExtLinux`, `LinuxClipboardKind`).
- On other platforms `Selection::Clipboard` uses the plain API, and every call with
  `Selection::Primary` fails with `the primary selection is not available on this platform`.
- A failure to open the clipboard is an error from every method.

### Test fake

`FakeSelections`, `#[cfg(test)]`, next to `FakeClipboard`:

- One `Content` per selection, `Content::Other` at the start. Clones share state.
- `set(selection, content)` and `get(selection)`.
- Switches `fail_read`, `fail_write` and `fail_clear`, each per selection.
- Counters `reads(selection)`, `writes(selection)` and `clears(selection)`.
- `write_text` stores `Content::Text`; `clear` stores `Content::Other`.

## Out of scope

- Detecting that a selection changed (ticket 0021).
- Reading or writing HTML, images or files.
- Any change to the TUI, which keeps using `Clipboard`.
- A binary that uses the new trait (tickets 0016 and 0020).

## Design notes

- **Files before text.** A file manager offers a file list and, usually, the path as plain
  text. Text first would turn a file copy into a text copy. A spreadsheet offers text and an
  image; that one must count as text, otherwise real data stays on the clipboard. Hence
  "files, then text", and no check for images.
- **`Other` is not an error.** An image, an empty selection and a selection without an owner
  all mean "nothing to obfuscate".
- **arboard reports a timeout the same way as missing text** (`ContentNotAvailable`). Such
  a read is `Other`. This is accepted.
- **No automated test reaches `SystemClipboard`.** The definition of done forbids touching
  a real clipboard. Each arboard call therefore stays a one-line mapping that can be
  reviewed by reading it; the logic is in `classify`. Ticket 0016's manual checklist is the
  first real use.
- **Check the arboard 3.7 documentation on docs.rs** for the exact builder calls
  (`get().clipboard(kind).text()`, `get().clipboard(kind).file_list()`,
  `set().clipboard(kind).text(..)`, `clear_with().clipboard(kind)`) and for the `cfg` under
  which the Linux extension traits exist. If `file_list` is missing on a platform that CI
  builds, treat the file probe as `Absent` there and say so in the report.
- Error messages are fixed texts or come from arboard. They never contain content.

## Acceptance criteria

Unit tests in `clipboard.rs`:

- [ ] `classify_files_win_over_text`: files found and text found gives `Files`.
- [ ] `classify_text`: no files and non-empty text gives `Text` with that text.
- [ ] `classify_no_text_is_other`: no files and absent text gives `Other`.
- [ ] `classify_empty_text_is_other`.
- [ ] `classify_failed_text_is_error`: the message is the fixed text above.
- [ ] `classify_failed_files_falls_through_to_text`.
- [ ] `content_debug_hides_text`: the `Debug` output of `Content::Text("secret".into())`
      does not contain `secret`.
- [ ] `fake_selections_are_independent`: writing one selection leaves the other unchanged.
- [ ] `fake_counts_reads_writes_and_clears`.
- [ ] `fake_failure_switches`: each switch makes exactly its operation fail on exactly its
      selection.

And:

- [ ] `grep -rn "SystemClipboard::new" crates/sagiri/src` shows no use inside a test module.
- [ ] The existing tests of `tui/app.rs` pass unchanged.
- [ ] `scripts/check.sh` passes.

## Test plan

- Table-driven unit tests for `classify`; unit tests for the fake.
- No test for `SystemClipboard`; the report states that its arboard calls were checked
  against the documentation.
