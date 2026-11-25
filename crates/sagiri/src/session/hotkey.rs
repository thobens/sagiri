//! The hotkey of `session.hotkey`: its syntax, and the pure helpers the X11 key grab
//! uses. Nothing here talks to a display, so the syntax is checked on every platform.

use std::fmt;

use sagiri_core::config::SessionConfig;

/// X11 modifier bits as they appear in a grab mask and in a key event's state.
const SHIFT_MASK: u16 = 1 << 0;
const LOCK_MASK: u16 = 1 << 1;
const CONTROL_MASK: u16 = 1 << 2;
const MOD1_MASK: u16 = 1 << 3;
/// Num Lock, assumed to be Mod2 as on every common setup.
const MOD2_MASK: u16 = 1 << 4;
const MOD4_MASK: u16 = 1 << 6;

/// The modifiers a hotkey can name. Every other bit of a key event's state is ignored.
const RELEVANT_MASK: u16 = SHIFT_MASK | CONTROL_MASK | MOD1_MASK | MOD4_MASK;

/// Presses closer together than this count as one (auto-repeat).
const DEBOUNCE_MS: u32 = 300;

/// A parsed key combination: four modifier flags and the X11 keysym of the key.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Hotkey {
    pub ctrl: bool,
    pub alt: bool,
    pub shift: bool,
    pub super_key: bool,
    pub keysym: u32,
}

impl Hotkey {
    /// The X11 modifier mask the combination needs.
    pub fn modifier_mask(&self) -> u16 {
        let mut mask = 0;
        if self.shift {
            mask |= SHIFT_MASK;
        }
        if self.ctrl {
            mask |= CONTROL_MASK;
        }
        if self.alt {
            mask |= MOD1_MASK;
        }
        if self.super_key {
            mask |= MOD4_MASK;
        }
        mask
    }
}

/// Why a hotkey text was refused. The texts are fixed and never repeat the input.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HotkeyError {
    Empty,
    NoKey,
    MoreThanOneKey,
    Unknown,
    RepeatedModifier,
    NeedsModifier,
}

impl fmt::Display for HotkeyError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            HotkeyError::Empty => "is empty or has an empty part",
            HotkeyError::NoKey => "names no key",
            HotkeyError::MoreThanOneKey => "names more than one key",
            HotkeyError::Unknown => "contains an unknown modifier or key",
            HotkeyError::RepeatedModifier => "repeats a modifier",
            HotkeyError::NeedsModifier => "needs ctrl, alt or super",
        })
    }
}

impl std::error::Error for HotkeyError {}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Modifier {
    Ctrl,
    Alt,
    Shift,
    Super,
}

fn modifier(word: &str) -> Option<Modifier> {
    match word {
        "ctrl" | "control" => Some(Modifier::Ctrl),
        "alt" => Some(Modifier::Alt),
        "shift" => Some(Modifier::Shift),
        "super" | "win" => Some(Modifier::Super),
        _ => None,
    }
}

/// The keysym of a key name, already in lower case.
fn keysym(word: &str) -> Option<u32> {
    let mut chars = word.chars();
    if let (Some(c), None) = (chars.next(), chars.next())
        && (c.is_ascii_lowercase() || c.is_ascii_digit())
    {
        // Letters (lower case) and digits are their ASCII code points.
        return Some(u32::from(c));
    }
    match word {
        "insert" => Some(0xff63),
        "pause" => Some(0xff13),
        _ => {
            let n: u32 = word.strip_prefix('f')?.parse().ok()?;
            // A leading zero or sign ("f01", "f+1") is not a key name.
            if !(1..=12).contains(&n) || word[1..] != n.to_string() {
                return None;
            }
            // F1 is 0xffbe, F12 is 0xffc9.
            Some(0xffbe + n - 1)
        }
    }
}

/// Parses a hotkey such as `ctrl+alt+o`.
pub fn parse(text: &str) -> Result<Hotkey, HotkeyError> {
    let mut modifiers: Vec<Modifier> = Vec::new();
    let mut key = None;
    for part in text.split('+') {
        let word = part.trim().to_ascii_lowercase();
        if word.is_empty() {
            return Err(HotkeyError::Empty);
        }
        if let Some(m) = modifier(&word) {
            if modifiers.contains(&m) {
                return Err(HotkeyError::RepeatedModifier);
            }
            modifiers.push(m);
        } else if let Some(sym) = keysym(&word) {
            if key.is_some() {
                return Err(HotkeyError::MoreThanOneKey);
            }
            key = Some(sym);
        } else {
            return Err(HotkeyError::Unknown);
        }
    }
    let keysym = key.ok_or(HotkeyError::NoKey)?;
    let has = |m| modifiers.contains(&m);
    let hotkey = Hotkey {
        ctrl: has(Modifier::Ctrl),
        alt: has(Modifier::Alt),
        shift: has(Modifier::Shift),
        super_key: has(Modifier::Super),
        keysym,
    };
    if !(hotkey.ctrl || hotkey.alt || hotkey.super_key) {
        return Err(HotkeyError::NeedsModifier);
    }
    Ok(hotkey)
}

/// Parses `session.hotkey` if it is set. The error names the setting, never its value.
pub fn from_config(settings: &SessionConfig) -> anyhow::Result<Option<Hotkey>> {
    match &settings.hotkey {
        None => Ok(None),
        Some(text) => match parse(text) {
            Ok(hotkey) => Ok(Some(hotkey)),
            Err(e) => anyhow::bail!("session.hotkey {e}"),
        },
    }
}

/// The keycodes whose first column in the server's keyboard mapping is `keysym`.
///
/// `keysyms` is the flat list of a `GetKeyboardMapping` reply that starts at
/// `min_keycode`, with `keysyms_per_keycode` entries per keycode.
pub fn keycodes_for(
    keysym: u32,
    min_keycode: u8,
    keysyms_per_keycode: u8,
    keysyms: &[u32],
) -> Vec<u8> {
    if keysyms_per_keycode == 0 {
        return Vec::new();
    }
    keysyms
        .chunks(usize::from(keysyms_per_keycode))
        .enumerate()
        .filter(|(_, row)| row.first() == Some(&keysym))
        .filter_map(|(i, _)| {
            u8::try_from(i)
                .ok()
                .and_then(|i| min_keycode.checked_add(i))
        })
        .collect()
}

/// The masks to grab: the modifiers alone, with Caps Lock, with Num Lock, and with both.
pub fn grab_masks(hotkey: &Hotkey) -> [u16; 4] {
    let mask = hotkey.modifier_mask();
    [
        mask,
        mask | LOCK_MASK,
        mask | MOD2_MASK,
        mask | LOCK_MASK | MOD2_MASK,
    ]
}

/// Whether a key press is the hotkey: one of its keycodes, and exactly its modifiers
/// among Shift, Control, Mod1 and Mod4.
pub fn matches(hotkey: &Hotkey, keycodes: &[u8], event_keycode: u8, event_state: u16) -> bool {
    keycodes.contains(&event_keycode) && event_state & RELEVANT_MASK == hotkey.modifier_mask()
}

/// Refuses a press that comes less than 300 ms after the last accepted one. Times are the
/// server's 32-bit milliseconds, compared with wrap-around.
#[derive(Debug, Default)]
pub struct Debounce {
    last: Option<u32>,
}

impl Debounce {
    pub fn new() -> Self {
        Debounce::default()
    }

    /// Whether the press at `time_ms` counts.
    pub fn accept(&mut self, time_ms: u32) -> bool {
        if let Some(last) = self.last
            && time_ms.wrapping_sub(last) < DEBOUNCE_MS
        {
            return false;
        }
        self.last = Some(time_ms);
        true
    }
}

/// Recognises the presses the X server's auto-repeat sends while a key is held.
///
/// The first repeat comes after the repeat delay (660 ms by default), which is longer
/// than `Debounce` waits. The server sends each repeat as a release and a press of the
/// same keycode with the same time; a press that follows such a release is a repeat.
#[derive(Debug, Default)]
pub struct AutoRepeat {
    last_release: Option<(u8, u32)>,
}

impl AutoRepeat {
    pub fn new() -> Self {
        AutoRepeat::default()
    }

    /// Notes a key release.
    pub fn released(&mut self, keycode: u8, time_ms: u32) {
        self.last_release = Some((keycode, time_ms));
    }

    /// Whether the press is a repeat of a held key.
    pub fn is_repeat(&self, keycode: u8, time_ms: u32) -> bool {
        self.last_release == Some((keycode, time_ms))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(ctrl: bool, alt: bool, shift: bool, super_key: bool, keysym: u32) -> Hotkey {
        Hotkey {
            ctrl,
            alt,
            shift,
            super_key,
            keysym,
        }
    }

    #[test]
    fn parses_ctrl_alt_o() {
        assert_eq!(parse("ctrl+alt+o"), Ok(key(true, true, false, false, 0x6f)));
    }

    #[test]
    fn parsing_ignores_case_and_spaces() {
        assert_eq!(
            parse("Ctrl + Alt + O"),
            Ok(key(true, true, false, false, 0x6f))
        );
        assert_eq!(
            parse(" SHIFT+super+A "),
            Ok(key(false, false, true, true, 0x61))
        );
    }

    #[test]
    fn aliases() {
        assert_eq!(parse("control+o"), parse("ctrl+o"));
        assert_eq!(parse("win+o"), parse("super+o"));
        assert_eq!(parse("win+o"), Ok(key(false, false, false, true, 0x6f)));
    }

    #[test]
    fn parses_function_and_named_keys() {
        assert_eq!(
            parse("super+f12"),
            Ok(key(false, false, false, true, 0xffc9))
        );
        assert_eq!(parse("alt+f1"), Ok(key(false, true, false, false, 0xffbe)));
        assert_eq!(
            parse("ctrl+insert"),
            Ok(key(true, false, false, false, 0xff63))
        );
        assert_eq!(
            parse("alt+pause"),
            Ok(key(false, true, false, false, 0xff13))
        );
        assert_eq!(parse("ctrl+7"), Ok(key(true, false, false, false, 0x37)));
        assert_eq!(parse("ctrl+0"), Ok(key(true, false, false, false, 0x30)));
        assert_eq!(parse("ctrl+z"), Ok(key(true, false, false, false, 0x7a)));
    }

    fn error_cases() -> Vec<(&'static str, &'static str)> {
        vec![
            ("", "is empty or has an empty part"),
            ("   ", "is empty or has an empty part"),
            ("ctrl++o", "is empty or has an empty part"),
            ("ctrl+alt+", "is empty or has an empty part"),
            ("ctrl+alt", "names no key"),
            ("ctrl+o+p", "names more than one key"),
            ("ctrl+hyper+o", "contains an unknown modifier or key"),
            ("ctrl+f13", "contains an unknown modifier or key"),
            ("ctrl+f01", "contains an unknown modifier or key"),
            ("ctrl+é", "contains an unknown modifier or key"),
            ("ctrl+control+o", "repeats a modifier"),
            ("shift+o", "needs ctrl, alt or super"),
            ("o", "needs ctrl, alt or super"),
        ]
    }

    #[test]
    fn rejects_each_error_case() {
        for (input, text) in error_cases() {
            let err = parse(input).expect_err(input);
            assert_eq!(err.to_string(), text, "{input:?}");
        }
    }

    #[test]
    fn error_text_never_contains_input() {
        let secrets = [
            "ctrl+hushword+o",
            "hushword",
            "ctrl+q+hushword",
            "Hush+o",
            "ctrl+alt+hush",
        ];
        for input in secrets {
            let msg = parse(input).unwrap_err().to_string();
            assert!(!msg.to_lowercase().contains("hush"), "{msg}");
        }
        let settings = SessionConfig {
            hotkey: Some("ctrl+hushword".to_string()),
            ..SessionConfig::default()
        };
        let msg = from_config(&settings).unwrap_err().to_string();
        assert_eq!(msg, "session.hotkey contains an unknown modifier or key");
        // Inputs that are made of known words only: the message names none of them.
        for (input, _) in error_cases() {
            let msg = parse(input).unwrap_err().to_string();
            for word in input.split('+').map(str::trim).filter(|w| w.len() > 1) {
                assert!(!msg.contains(word), "{input:?}: {msg}");
            }
        }
    }

    #[test]
    fn from_config_without_hotkey_is_none() {
        assert_eq!(from_config(&SessionConfig::default()).unwrap(), None);
    }

    #[test]
    fn keycodes_for_keysym() {
        let o = 0x6f;
        // Keycodes 8 to 12, three columns each.
        let keysyms = [
            0x61, 0x41, 0, // 8: a A
            o, 0x4f, 0, // 9: o O
            0x70, o, 0, // 10: o only in the second column
            o, 0, 0, // 11: o
            0x71, 0x51, 0, // 12: q Q
        ];
        assert_eq!(keycodes_for(o, 8, 3, &keysyms), vec![9, 11]);
        assert_eq!(keycodes_for(0xffc9, 8, 3, &keysyms), Vec::<u8>::new());
        assert_eq!(keycodes_for(o, 8, 0, &keysyms), Vec::<u8>::new());
    }

    #[test]
    fn grab_masks_cover_lock_variants() {
        let hotkey = parse("ctrl+alt+o").unwrap();
        let base = CONTROL_MASK | MOD1_MASK;
        assert_eq!(
            grab_masks(&hotkey),
            [
                base,
                base | LOCK_MASK,
                base | MOD2_MASK,
                base | LOCK_MASK | MOD2_MASK
            ]
        );
        let hotkey = parse("super+shift+f1").unwrap();
        assert_eq!(grab_masks(&hotkey)[0], MOD4_MASK | SHIFT_MASK);
    }

    #[test]
    fn match_ignores_numlock_and_capslock() {
        let hotkey = parse("ctrl+alt+o").unwrap();
        let codes = [32, 90];
        let base = CONTROL_MASK | MOD1_MASK;
        for state in [
            base,
            base | LOCK_MASK,
            base | MOD2_MASK,
            base | LOCK_MASK | MOD2_MASK,
        ] {
            assert!(matches(&hotkey, &codes, 32, state), "{state:#x}");
            assert!(matches(&hotkey, &codes, 90, state), "{state:#x}");
        }
        assert!(!matches(&hotkey, &codes, 33, base));
    }

    #[test]
    fn match_requires_exact_modifiers() {
        let hotkey = parse("ctrl+alt+o").unwrap();
        let codes = [32];
        let base = CONTROL_MASK | MOD1_MASK;
        assert!(!matches(&hotkey, &codes, 32, base | SHIFT_MASK));
        assert!(!matches(&hotkey, &codes, 32, base | MOD4_MASK));
        assert!(!matches(&hotkey, &codes, 32, CONTROL_MASK));
        assert!(!matches(&hotkey, &codes, 32, 0));
    }

    #[test]
    fn repeat_within_300ms_is_ignored() {
        let mut d = Debounce::new();
        assert!(d.accept(1_000));
        assert!(!d.accept(1_100));
        assert!(!d.accept(1_299));
        // Measured from the last accepted press, not from the refused ones.
        assert!(d.accept(1_300));
        assert!(!d.accept(1_599));
        assert!(d.accept(2_000));
    }

    #[test]
    fn auto_repeat_is_recognised() {
        let mut r = AutoRepeat::new();
        assert!(!r.is_repeat(32, 1_000));
        // The server's repeat: release and press of the same keycode at the same time.
        r.released(32, 1_660);
        assert!(r.is_repeat(32, 1_660));
        // A real release, then a new press later.
        r.released(32, 2_100);
        assert!(!r.is_repeat(32, 2_600));
        // Another key released at the same time does not make a press a repeat.
        r.released(37, 3_000);
        assert!(!r.is_repeat(32, 3_000));
    }

    #[test]
    fn debounce_survives_time_wraparound() {
        let mut d = Debounce::new();
        assert!(d.accept(u32::MAX - 100));
        // 150 ms later, after the wrap.
        assert!(!d.accept(49));
        // 400 ms after the previous press.
        assert!(d.accept(449));
        let mut d = Debounce::new();
        assert!(d.accept(u32::MAX - 10));
        assert!(d.accept(290));
    }
}
