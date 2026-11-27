---
id: "0039"
title: System tray icon
status: done
depends_on: ["0022", "0023"]
---

# 0039 — System tray icon

## Why

A notification says that a session started or stopped, and then it is gone. Nothing on
screen shows the state afterwards, so the user has to remember it or run
`sagiri-session status`. A tray icon shows the state for as long as the session process
runs, and a click on it switches the session without a hotkey.

## Scope

In `crates/sagiri` and `crates/sagiri-core`. The icon belongs to `sagiri-session run`; no
new binary and no new dependency.

### Setting (`crates/sagiri-core/src/config.rs`)

- `SessionConfig` gains `pub tray: bool`, default `true`. The key is `tray` in `[session]`.
- A value that is not a boolean is refused with `` `session.tray`: expected a boolean ``.

### Pure part (`src/session/tray.rs`, every platform)

- `TrayState`: what the icon shows. Shared between the session thread and the tray thread.
  - `TrayState::new() -> Arc<TrayState>` starts inactive.
  - `is_active()`, `set_active(bool)`.
  - `set_waker(f)`: `f` runs after every call of `set_active` that changes the state.
    Without a waker a change is only stored.
- `TrayNotifier`: a `Notifier` that feeds a `TrayState`.

  | Notice | State afterwards |
  |---|---|
  | `Started` | active |
  | `Stopped`, `BackendLost` | inactive |
  | every other notice | unchanged |

- `icon(width, height, active) -> Icon`: the picture for a window of that size, as one
  colour and a list of rectangles. It is three bars, like lines of text, in the largest
  square that fits, centred.

  | State | Bars | Colour |
  |---|---|---|
  | inactive | thin | grey `#909090` |
  | active | thick, like redacted lines | green `#2fb344` |

  The two states differ in shape, not only in colour. A window with a side of 0 gives no
  rectangles.
- Helpers of the tray protocol, all pure:
  - `selection_name(screen) -> String`: `_NET_SYSTEM_TRAY_S<screen>`.
  - `dock_request(window) -> [u32; 5]`: the data of the `SYSTEM_TRAY_REQUEST_DOCK`
    message.
  - `XEMBED_INFO: [u32; 2]`: protocol version 0 and the flag `XEMBED_MAPPED`.
  - `announced_manager(data, selection) -> Option<u32>`: the new tray manager named by a
    `MANAGER` client message, if the message is about `selection`.
  - `alpha_bits(depth, red_mask, green_mask, blue_mask) -> u32`: the bits of a pixel that
    hold its opacity, which are all bits outside the colour masks on a 32-bit visual and
    none otherwise.
- `Rgb::channels16()`: the channels widened to the sixteen bits of the X colour requests.

### X11 part (`src/session/x11_tray.rs`, Linux)

- `pub fn start(events: Sender<Event>, state: Arc<TrayState>) -> anyhow::Result<()>`
  opens its own connection and starts a thread named `tray`. Connection and set-up errors
  are returned before the thread runs.
- The thread implements the freedesktop System Tray Protocol (XEmbed), client side:
  - It looks up the owner of `_NET_SYSTEM_TRAY_S<screen>`. Without an owner it waits for
    a `MANAGER` message on the root window, so a tray that starts later, or restarts, gets
    the icon too.
  - For a manager it creates the icon window with the visual from the manager's
    `_NET_SYSTEM_TRAY_VISUAL` property (the root visual if there is none), sets
    `_XEMBED_INFO`, the window name `Sagiri` (`WM_NAME` and `_NET_WM_NAME`) and `WM_CLASS`
    with the instance `sagiri-session` and the class `Sagiri`, and sends the dock request.
  - The window is override-redirect, so that no window manager takes it while it is not
    embedded.
  - The background is transparent on a 32-bit visual and `ParentRelative` otherwise, so
    the bar shows through around the bars of the icon.
  - It draws `icon(..)` for the current window size on `Expose`, after a change of the
    window size, and when the waker of `TrayState` fires.
  - A press of mouse button 1 on the icon sends `Event::Toggle`. Other buttons do nothing.
  - When the manager's window is destroyed, or the icon window is destroyed or handed
    back to the root window, the thread destroys the icon window and waits for the next
    manager. A manager that hands the icon back is not asked again until it announces
    itself again.
- The thread never reads or writes a selection and sends no event other than
  `Event::Toggle`. It ends silently when its connection ends: the watcher of ticket 0021
  reports the lost display.
- The waker is in place when `start` returns, so that `--active` reaches the icon. It is
  removed again if the thread cannot be spawned and when the thread ends, so that no
  change of the state writes to a connection that nobody reads.

### Wiring

- `backend::start` takes the tray state as a third argument, `Option<Arc<TrayState>>`.
  On X11 it starts the watcher and then, with a state, the tray thread.
- If the tray thread cannot be started, `sagiri-session run` writes
  `sagiri-session: the tray icon is not available: <reason>` to standard error once and
  keeps running. A missing tray manager is not an error and prints nothing.
- `sagiri-session run` creates the state and adds a `TrayNotifier` to the notifiers when
  `session.tray` is `true`. With `tray = false` no second connection is opened.

### Documentation

- `README.md`, "Session mode": what the icon shows, the click, which trays show it (those
  that speak XEmbed: i3bar, polybar, the Xfce and Plasma panels, stalonetray, trayer),
  and that a desktop without such a tray shows nothing.
- `README.md`, "Session settings": the key `tray`.
- `tickets/README.md`: index, order, layout, pinned API, backlog.

## Out of scope

- A menu on the icon, a tooltip, a click that quits the process.
- The StatusNotifierItem protocol (D-Bus), and with it GNOME Shell and Wayland bars.
- An icon state for failures such as a cleared clipboard.
- Icon colours or an icon file in the config.
- A tray icon on macOS.

## Design notes

- **XEmbed instead of StatusNotifierItem.** Session mode is X11 only, `x11rb` is already
  a dependency, and i3bar shows XEmbed icons only. StatusNotifierItem needs a D-Bus
  client crate with an async runtime, which ticket 0022 already decided against.
- **An own connection and thread.** The watcher of ticket 0021 blocks on its connection
  and its loop is timing-sensitive in watch mode. The tray has nothing in common with it
  but the display.
- **The state travels through the notifier.** `Session` already tells its `Notifier`
  about every start and stop, whatever caused it: hotkey, socket, click or `--active`.
  `Notice` has no text payload, so the icon cannot show content by construction.
- **Waking a thread that blocks on X.** `RustConnection` can be used from several
  threads. The waker sends a client message to a window of the tray connection that is
  never mapped, and the tray thread redraws when it reads it. No polling.
- **Rectangles instead of an image.** `PolyFillRectangle` with an allocated colour works
  on every visual and needs no knowledge of pixel formats or byte order. Older X servers
  allocate a colour of a 32-bit visual with the opacity bits at zero, so `alpha_bits` is
  added to the pixel.
- **The tray is an indicator, not a guard.** A tray that cannot be started must not stop
  session mode, so that failure is a warning.
- **Testing.** The pure part has unit tests. The X11 part is tested under Xvfb by
  `scripts/x11-test.sh`, with a small tray manager inside the test: it owns the
  selection, embeds, resizes and releases the icon window and reads its pixels. A resize
  also exposes the whole window (bit gravity `Forget`), so the test of a resize shows that
  the size is updated, but passes as well without the redraw after `ConfigureNotify`.

## Acceptance criteria

Unit tests in `config.rs`:

- [ ] `session_defaults` and `session_section_parses` cover `tray`.
- [ ] `wrong_type_for_hotkey_and_notify_is_error` covers `session.tray`.

Unit tests in `tray.rs`:

- [ ] `state_starts_inactive`.
- [ ] `notices_set_the_state`: the table above, over all six notices.
- [ ] `waker_runs_on_change_only`: three calls of `set_active`, two of them changes, run
      the waker twice.
- [ ] `replaced_waker_is_dropped`: a waker that `set_waker` replaces is dropped, with
      what it holds, and only the new one runs.
- [ ] `icon_stays_inside_the_window`: for every size from 0x0 to 64x64 and both states,
      every rectangle is inside the window and none is empty.
- [ ] `icon_is_a_centred_square`: in a 48x20 window all rectangles lie in the middle
      20 columns.
- [ ] `states_differ_in_shape`: from 12x12 on, the active bars are thicker than the
      inactive ones, and the colours differ.
- [ ] `tiny_or_empty_window_gives_no_panic`: sizes 0 to 7 in both directions.
- [ ] `protocol_helpers`: `selection_name`, `dock_request`, `XEMBED_INFO`,
      `announced_manager` (matching and other selection, owner 0), `alpha_bits` (depth 24
      and 32).
- [ ] `colours_widen_to_sixteen_bits`.

Unit test in `notify.rs`:

- [ ] `tray_false_adds_no_notifier`: `Notifiers::for_session`, which takes the
      `SessionConfig` and an optional tray state, returns one more notifier with a state,
      and that notifier feeds the state.

Unit test in `backend.rs`:

- [ ] `tray_warning_names_the_reason`: the warning line that `run` writes when the tray
      thread cannot be started, with the whole chain of the error.

Tests in `crates/sagiri/tests/session_x11.rs` (ignored, run by `scripts/x11-test.sh`):

- [ ] `x11_tray_docks_with_running_manager`: exactly one dock request arrives, the
      window carries `_XEMBED_INFO` with the flag `XEMBED_MAPPED`, has the root visual
      and is override-redirect, `WM_NAME` and `_NET_WM_NAME` are `Sagiri`, and
      `WM_CLASS` is `sagiri-session\0Sagiri\0`.
- [ ] `x11_tray_docks_with_later_manager`: the manager starts after the session process.
- [ ] `x11_tray_click_toggles`: a click with button 1 on the embedded icon switches the
      session on, a second one off. A click with button 3 and one with button 2 change
      nothing, each checked on its own.
- [ ] `x11_tray_icon_shows_state`: the embedded window shows the inactive colour, after
      `start` only the active colour on more pixels, and after `stop` the first picture
      again. The bar pixels match the area of `icon(..)`, and every other pixel shows the
      tray's background, which is not 0 (`ParentRelative`).
- [ ] `x11_tray_icon_uses_the_manager_visual`: for a manager that names a 32-bit visual
      the icon window has that visual, and the same picture test passes with opaque
      pixels on a transparent background (pixel value 0).
- [ ] `x11_tray_icon_follows_resize`: the tray sizes the icon 40x40, then 48x30, and in
      both states the picture matches `icon(..)` for that size.
- [ ] `x11_tray_redocks_after_release`: a running tray hands the icon back to the root
      window. The icon window is destroyed, no dock request follows until the tray
      announces itself again, and then one with a new window arrives.
- [ ] `x11_tray_redocks_after_manager_restart`: after the manager ends and another one
      announces itself, a second dock request arrives, the old icon window is gone, and
      a click on the new icon works. Run twice: with the icon destroyed along with the
      manager, and with the icon in the manager's save-set, which hands it back to the
      root window.
- [ ] `x11_tray_off_requests_no_dock`: with `tray = false` no dock request arrives.

And:

- [ ] `README.md` documents the icon and the setting.
- [ ] `scripts/check.sh` passes.
- [ ] `scripts/x11-test.sh` passes.

Manual check on a desktop with a tray (listed in the report as done or not possible):

- [ ] On i3 with i3bar the icon appears in the bar, a click switches the session and the
      icon changes, and after `i3-msg restart` the icon is back.

## Test plan

- Unit tests as named above.
- `scripts/x11-test.sh` for the Xvfb tests.
- The manual check.
