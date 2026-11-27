//! The tray icon of session mode, as far as it needs no display: the state the icon
//! shows, the notifier that keeps that state current, the picture, and the fixed values
//! of the System Tray Protocol. The thread that talks to the X server is in `x11_tray`.
//!
//! The icon shows whether a session is active and nothing else. Nothing here holds or
//! receives clipboard text.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use super::notify::{Notice, Notifier};

/// Runs after a change of the state, on the thread that changed it.
type Waker = Box<dyn Fn() + Send + Sync>;

/// What the tray icon shows. Written by the session thread, read by the tray thread.
pub struct TrayState {
    active: AtomicBool,
    waker: Mutex<Option<Waker>>,
}

impl TrayState {
    /// A new state. It starts inactive, like a session.
    pub fn new() -> Arc<Self> {
        Arc::new(Self {
            active: AtomicBool::new(false),
            waker: Mutex::new(None),
        })
    }

    /// Whether the icon shows an active session.
    pub fn is_active(&self) -> bool {
        self.active.load(Ordering::SeqCst)
    }

    /// Sets what the icon shows. A change runs the waker; setting the current state again
    /// does nothing.
    pub fn set_active(&self, active: bool) {
        if self.active.swap(active, Ordering::SeqCst) == active {
            return;
        }
        // A poisoned lock means that a waker panicked. The state is stored either way.
        if let Ok(waker) = self.waker.lock()
            && let Some(wake) = waker.as_ref()
        {
            wake();
        }
    }

    /// Sets what runs after every change of the state. It replaces an earlier waker, and
    /// must not call `set_waker` or `set_active` itself.
    pub fn set_waker(&self, waker: impl Fn() + Send + Sync + 'static) {
        if let Ok(mut slot) = self.waker.lock() {
            *slot = Some(Box::new(waker));
        }
    }
}

/// Keeps a `TrayState` current with what the session tells its notifiers.
pub struct TrayNotifier(Arc<TrayState>);

impl TrayNotifier {
    /// A notifier that feeds `state`.
    pub fn new(state: Arc<TrayState>) -> Self {
        Self(state)
    }
}

impl Notifier for TrayNotifier {
    fn notify(&mut self, notice: Notice) {
        match notice {
            Notice::Started => self.0.set_active(true),
            Notice::Stopped | Notice::BackendLost => self.0.set_active(false),
            Notice::ClipboardCleared
            | Notice::OriginalStillOnClipboard
            | Notice::PrimaryNotUpdated => {}
        }
    }
}

/// A colour with eight bits per channel.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Rgb {
    pub red: u8,
    pub green: u8,
    pub blue: u8,
}

impl Rgb {
    /// The channels with sixteen bits each, as the X colour requests take them.
    pub fn channels16(self) -> (u16, u16, u16) {
        let wide = |channel: u8| u16::from(channel) * 257;
        (wide(self.red), wide(self.green), wide(self.blue))
    }
}

/// The colour of the icon while a session is active.
pub const ACTIVE: Rgb = Rgb {
    red: 0x2f,
    green: 0xb3,
    blue: 0x44,
};

/// The colour of the icon while no session is active.
pub const INACTIVE: Rgb = Rgb {
    red: 0x90,
    green: 0x90,
    blue: 0x90,
};

/// A filled rectangle of the icon, in window coordinates.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Bar {
    pub x: i16,
    pub y: i16,
    pub width: u16,
    pub height: u16,
}

/// The picture of the icon: bars of one colour on a background that is left alone.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Icon {
    pub color: Rgb,
    pub bars: Vec<Bar>,
}

/// The lengths of the three bars, in eighths of the widest one.
const LENGTHS: [u32; 3] = [8, 5, 7];

/// The icon for a window of `width` by `height` pixels: three bars like lines of text,
/// in the largest square that fits, centred. Thin grey bars for an inactive session,
/// thick green ones, like redacted lines, for an active one. A square too small for three
/// bars gets one block, and an empty window gets nothing.
pub fn icon(width: u16, height: u16, active: bool) -> Icon {
    let color = if active { ACTIVE } else { INACTIVE };
    let (width, height) = (u32::from(width), u32::from(height));
    let side = width.min(height);
    let left = (width - side) / 2;
    let top = (height - side) / 2;
    let pad = side / 8;
    let inner = side - 2 * pad;
    let pitch = inner / 3;

    let mut bars = Vec::new();
    let mut push = |x: u32, y: u32, width: u32, height: u32| {
        if let (Ok(x), Ok(y), Ok(width), Ok(height)) = (
            i16::try_from(x),
            i16::try_from(y),
            u16::try_from(width),
            u16::try_from(height),
        ) && width > 0
            && height > 0
        {
            bars.push(Bar {
                x,
                y,
                width,
                height,
            });
        }
    };

    if pitch == 0 {
        push(left + pad, top + pad, inner, inner);
        return Icon { color, bars };
    }
    let thickness = if active {
        (pitch - (pitch / 4).max(1)).max(1)
    } else {
        (side / 12).max(1).min(pitch)
    };
    // The rows share what three equal rows leave of the inner square.
    let first_row = top + pad + (inner - 3 * pitch) / 2;
    for (row, length) in (0u32..).zip(LENGTHS) {
        push(
            left + pad,
            first_row + row * pitch + (pitch - thickness) / 2,
            (inner * length / 8).max(1),
            thickness,
        );
    }
    Icon { color, bars }
}

/// The name of the selection that the tray manager of `screen` owns.
pub fn selection_name(screen: usize) -> String {
    format!("_NET_SYSTEM_TRAY_S{screen}")
}

/// The opcode of a `_NET_SYSTEM_TRAY_OPCODE` message that asks to embed a window.
const SYSTEM_TRAY_REQUEST_DOCK: u32 = 0;

/// The data of the message that asks a tray manager to embed `window`: the time (zero for
/// "now"), the opcode and the window.
pub fn dock_request(window: u32) -> [u32; 5] {
    [0, SYSTEM_TRAY_REQUEST_DOCK, window, 0, 0]
}

/// The flag of `_XEMBED_INFO` that asks the embedder to map the window.
const XEMBED_MAPPED: u32 = 1;

/// The value of the `_XEMBED_INFO` property: protocol version 0, and the window wants to
/// be mapped.
pub const XEMBED_INFO: [u32; 2] = [0, XEMBED_MAPPED];

/// The tray manager that a `MANAGER` client message announces, if the message is about
/// `selection`. The data of such a message is the time, the selection and its new owner.
pub fn announced_manager(data: [u32; 5], selection: u32) -> Option<u32> {
    (data[1] == selection && data[2] != 0).then_some(data[2])
}

/// The bits of a pixel that hold its opacity. On a visual of depth 32 these are the bits
/// outside the colour masks; older X servers allocate a colour with them cleared, and
/// such a pixel is invisible. Other visuals have no such bits.
pub fn alpha_bits(depth: u8, red_mask: u32, green_mask: u32, blue_mask: u32) -> u32 {
    if depth == 32 {
        !(red_mask | green_mask | blue_mask)
    } else {
        0
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::AtomicUsize;

    const ALL: [Notice; 6] = [
        Notice::Started,
        Notice::Stopped,
        Notice::ClipboardCleared,
        Notice::OriginalStillOnClipboard,
        Notice::PrimaryNotUpdated,
        Notice::BackendLost,
    ];

    #[test]
    fn state_starts_inactive() {
        assert!(!TrayState::new().is_active());
    }

    #[test]
    fn notices_set_the_state() {
        use Notice::*;
        let table = [
            (Started, Some(true)),
            (Stopped, Some(false)),
            (ClipboardCleared, None),
            (OriginalStillOnClipboard, None),
            (PrimaryNotUpdated, None),
            (BackendLost, Some(false)),
        ];
        assert_eq!(table.map(|(notice, _)| notice), ALL);
        for (notice, expected) in table {
            for before in [false, true] {
                let state = TrayState::new();
                state.set_active(before);
                TrayNotifier::new(Arc::clone(&state)).notify(notice);
                assert_eq!(
                    state.is_active(),
                    expected.unwrap_or(before),
                    "{notice:?} from {before}"
                );
            }
        }
    }

    #[test]
    fn waker_runs_on_change_only() {
        let state = TrayState::new();
        // Without a waker a change is only stored.
        state.set_active(true);
        state.set_active(false);
        let runs = Arc::new(AtomicUsize::new(0));
        let counter = Arc::clone(&runs);
        state.set_waker(move || {
            counter.fetch_add(1, Ordering::SeqCst);
        });
        state.set_active(true);
        state.set_active(true);
        state.set_active(false);
        assert_eq!(runs.load(Ordering::SeqCst), 2);
        assert!(!state.is_active());
    }

    #[test]
    fn replaced_waker_is_dropped() {
        let state = TrayState::new();
        // Stands in for the connection that the waker of the tray thread holds.
        let held = Arc::new(());
        let captured = Arc::clone(&held);
        state.set_waker(move || {
            let _ = &captured;
        });
        assert_eq!(Arc::strong_count(&held), 2);
        let runs = Arc::new(AtomicUsize::new(0));
        let counter = Arc::clone(&runs);
        state.set_waker(move || {
            counter.fetch_add(1, Ordering::SeqCst);
        });
        assert_eq!(Arc::strong_count(&held), 1);
        // Only the new waker runs.
        state.set_active(true);
        assert_eq!(runs.load(Ordering::SeqCst), 1);
    }

    /// The bars of `icon` as (left, top, right, bottom), right and bottom exclusive.
    fn edges(icon: &Icon) -> Vec<(i32, i32, i32, i32)> {
        icon.bars
            .iter()
            .map(|b| {
                let (x, y) = (i32::from(b.x), i32::from(b.y));
                (x, y, x + i32::from(b.width), y + i32::from(b.height))
            })
            .collect()
    }

    #[test]
    fn icon_stays_inside_the_window() {
        for width in 0..=64u16 {
            for height in 0..=64u16 {
                for active in [false, true] {
                    let icon = icon(width, height, active);
                    for (left, top, right, bottom) in edges(&icon) {
                        let at = format!("{width}x{height} active={active}");
                        assert!(left >= 0 && top >= 0, "{at}");
                        assert!(right > left && bottom > top, "{at}");
                        assert!(right <= i32::from(width), "{at}");
                        assert!(bottom <= i32::from(height), "{at}");
                    }
                    if width >= 3 && height >= 3 {
                        assert!(!icon.bars.is_empty(), "{width}x{height}");
                    }
                }
            }
        }
    }

    #[test]
    fn icon_is_a_centred_square() {
        for active in [false, true] {
            let wide = icon(48, 20, active);
            assert_eq!(wide.bars.len(), 3);
            for (left, _, right, _) in edges(&wide) {
                assert!(left >= 14 && right <= 34, "{left}..{right}");
            }
            let tall = icon(20, 48, active);
            for (_, top, _, bottom) in edges(&tall) {
                assert!(top >= 14 && bottom <= 34, "{top}..{bottom}");
            }
            // The picture is the same, only moved.
            let square = icon(20, 20, active);
            let moved: Vec<Bar> = square
                .bars
                .iter()
                .map(|b| Bar { x: b.x + 14, ..*b })
                .collect();
            assert_eq!(wide.bars, moved);
        }
    }

    #[test]
    fn states_differ_in_shape() {
        assert_ne!(ACTIVE, INACTIVE);
        assert_eq!(icon(24, 24, true).color, ACTIVE);
        assert_eq!(icon(24, 24, false).color, INACTIVE);
        for side in 12..=64u16 {
            let (on, off) = (icon(side, side, true), icon(side, side, false));
            assert_eq!(on.bars.len(), 3, "{side}");
            assert_eq!(off.bars.len(), 3, "{side}");
            for (thick, thin) in on.bars.iter().zip(&off.bars) {
                assert!(thick.height > thin.height, "{side}");
                // Same lines of text, redacted or not.
                assert_eq!((thick.x, thick.width), (thin.x, thin.width), "{side}");
            }
            // The bars of one icon do not touch.
            for pair in edges(&on).windows(2) {
                assert!(pair[0].3 < pair[1].1, "{side}");
            }
        }
    }

    #[test]
    fn tiny_or_empty_window_gives_no_panic() {
        for width in 0..8u16 {
            for height in 0..8u16 {
                for active in [false, true] {
                    let icon = icon(width, height, active);
                    if width == 0 || height == 0 {
                        assert!(icon.bars.is_empty(), "{width}x{height}");
                    }
                }
            }
        }
        // The largest window the protocol can describe.
        for active in [false, true] {
            let _ = icon(u16::MAX, u16::MAX, active);
            let _ = icon(u16::MAX, 1, active);
            let _ = icon(1, u16::MAX, active);
        }
    }

    #[test]
    fn protocol_helpers() {
        assert_eq!(selection_name(0), "_NET_SYSTEM_TRAY_S0");
        assert_eq!(selection_name(2), "_NET_SYSTEM_TRAY_S2");
        assert_eq!(dock_request(0x60_0001), [0, 0, 0x60_0001, 0, 0]);
        assert_eq!(XEMBED_INFO, [0, 1]);

        let selection = 400;
        assert_eq!(
            announced_manager([7, selection, 0x20_0003, 0, 0], selection),
            Some(0x20_0003)
        );
        // Another selection got a new owner.
        assert_eq!(
            announced_manager([7, 401, 0x20_0003, 0, 0], selection),
            None
        );
        // No owner is no manager.
        assert_eq!(announced_manager([7, selection, 0, 0, 0], selection), None);

        assert_eq!(alpha_bits(24, 0xff_0000, 0xff00, 0xff), 0);
        assert_eq!(alpha_bits(32, 0xff_0000, 0xff00, 0xff), 0xff00_0000);
        assert_eq!(alpha_bits(32, 0xff, 0xff00, 0xff_0000), 0xff00_0000);
        assert_eq!(alpha_bits(16, 0xf800, 0x07e0, 0x001f), 0);
    }

    #[test]
    fn colours_widen_to_sixteen_bits() {
        let colour = Rgb {
            red: 0,
            green: 0x80,
            blue: 0xff,
        };
        assert_eq!(colour.channels16(), (0, 0x8080, 0xffff));
    }
}
