---
id: "0031"
title: Leave an image alone when its owner answers a text request with other data
status: todo
depends_on: ["0021"]
---

# 0031 — Leave an image alone when its owner answers a text request with other data

## Why

Session mode promises that images and files on the clipboard are left untouched. That holds
for a program that follows the X11 selection convention (ICCCM): asked for a text target it
does not offer, it refuses, arboard reports `ContentNotAvailable`, and the selection is
classified as `Content::Other`.

Some programs do not refuse. `xclip -selection clipboard -t image/png -i file.png` answers
every conversion request, including `UTF8_STRING`, with the PNG bytes. The text read then
fails with an error other than `ContentNotAvailable`, `probe` maps it to `Probe::Failed`,
`classify` turns a failed text probe into an error, and the session fails closed: it clears
the clipboard. The copied image is lost.

Found while running the manual checks of ticket 0021 and reproduced by its reviewer on a
private Xvfb display, on `master` at commit `32eeb0d`:

| Clipboard owner | Result today |
|---|---|
| GTK3 program offering only image targets | untouched, no message |
| `xclip -t image/png` holding a valid PNG, session active | clipboard cleared, message "the clipboard could not be replaced and was cleared" |
| the same owner, `sagiri-cli --in clipboard` | exit 1, "the selection could not be read as text" |
| `xclip -t text/uri-list` holding a file list | untouched |

The second and third rows are the bug.

The error arboard returns in this case was not captured. The reviewer's reading of
arboard 3.7.0 (`src/platform/linux/x11.rs`) points to
`Error::Unknown("incorrect type received from clipboard")`. The first step of this ticket
is to confirm that with a test owner.

## Scope

### `crates/sagiri/src/clipboard.rs`

- A selection whose owner does not offer text is classified as `Content::Other`, also when
  that owner answers a text request with data of another type.
- A selection whose owner offers text and whose text read fails stays an error. The session
  keeps failing closed for it.
- `classify` stays a pure function that is tested without a clipboard. If it needs a third
  input, or `Probe` needs another variant, the unit tests cover every combination.

### Tests

- Unit tests for the new classification rule.
- A reproduction against a real X server belongs to the script of ticket 0026 if that
  ticket is merged when this one starts; otherwise it is a manual check.

## Out of scope

- Obfuscating text inside images, or any other handling of image content.
- Changes to how files (`text/uri-list`) are classified.
- Wayland and macOS behaviour, beyond keeping the existing tests green.
- Replacing arboard.

## Design notes

- **Fail closed stays the rule for text.** Treating every failed text read as
  `Content::Other` would fix the image case and open a hole: a text owner whose read fails
  would keep the original on the clipboard while the session is active. The decision has to
  rest on what the owner offers, not on the read having failed.
- **One way to decide:** ask the owner for `TARGETS` before reading text. An owner that
  lists no text target (`UTF8_STRING`, `STRING`, `TEXT`, `text/plain`,
  `text/plain;charset=utf-8`) holds no text. `x11rb` is a direct dependency since ticket
  0021. Whether the query goes through arboard or `x11rb` is the implementer's choice; an
  owner that does not answer `TARGETS` at all is treated as offering text.
- **Another way:** tell the "incorrect type" error apart from other failures. This depends
  on the text of an `Error::Unknown` message, which arboard does not guarantee. It is
  acceptable only if a test pins the dependency version behaviour and the fallback for an
  unrecognized error is the existing fail-closed path.
- **No content in messages.** The existing messages name no clipboard content. That does
  not change.

## Acceptance criteria

- [ ] A unit test shows that an owner offering only non-text targets is classified as
      `Content::Other`, whatever the text read returns.
- [ ] A unit test shows that an owner offering a text target whose read fails still gives
      the error "the selection could not be read as text".
- [ ] Every existing test of `classify` and of the session core passes, changed only where
      the signature of `classify` or `Probe` changed.
- [ ] Manual check on a private Xvfb display, or in the script of ticket 0026: with the
      session active, a PNG copied with `xclip -selection clipboard -t image/png` is still
      served unchanged afterwards and no message is printed.
- [ ] The same owner with `sagiri-cli --in clipboard`: the outcome is the one ticket 0016
      defines for a selection without text, not "could not be read as text".
- [ ] Manual check: text copied with `xclip` and with `xsel` is still obfuscated while the
      session is active.
- [ ] `scripts/check.sh` passes.

## Test plan

- Table-driven unit tests next to the existing `classify` tests.
- Manual checks with `Xvfb`, `xclip` and `xsel` on a private display, never on the real
  desktop clipboard.
