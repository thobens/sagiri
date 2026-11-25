//! The seam to the display: the backend watches the selections and sends
//! `Event::Changed` and `Event::BackendLost`.

use std::sync::mpsc::Sender;

use sagiri_core::config::SessionConfig;
use sagiri_core::paths::EnvLookup;

use super::core::Event;

/// The display backend session mode can use.
#[derive(Debug, PartialEq, Eq)]
pub enum Backend {
    /// An X11 display, watched through the XFixes extension.
    X11,
    /// No usable backend; the reason is shown to the user.
    Unsupported(&'static str),
}

/// Chooses the backend from the platform and the environment.
pub fn detect(env: EnvLookup<'_>) -> Backend {
    if !cfg!(target_os = "linux") {
        return Backend::Unsupported("session mode needs Linux with X11");
    }
    let set = |name: &str| env(name).is_some_and(|value| !value.is_empty());
    if set("WAYLAND_DISPLAY") {
        Backend::Unsupported("Wayland sessions are not supported yet")
    } else if !set("DISPLAY") {
        Backend::Unsupported("no X display: DISPLAY is not set")
    } else {
        Backend::X11
    }
}

/// Starts the display backend, which sends its events on `events`.
pub fn start(events: Sender<Event>, settings: &SessionConfig) -> anyhow::Result<()> {
    match detect(&|name| std::env::var_os(name)) {
        Backend::Unsupported(reason) => anyhow::bail!("session mode is not available: {reason}"),
        Backend::X11 => start_x11(events, settings),
    }
}

#[cfg(target_os = "linux")]
fn start_x11(events: Sender<Event>, settings: &SessionConfig) -> anyhow::Result<()> {
    super::x11::start(events, settings)
}

#[cfg(not(target_os = "linux"))]
fn start_x11(_events: Sender<Event>, _settings: &SessionConfig) -> anyhow::Result<()> {
    anyhow::bail!("session mode is not available: session mode needs Linux with X11")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    use std::ffi::OsString;

    fn detect_in(vars: &[(&str, &str)]) -> Backend {
        let map: HashMap<String, OsString> = vars
            .iter()
            .map(|(k, v)| ((*k).to_string(), OsString::from(v)))
            .collect();
        detect(&|name| map.get(name).cloned())
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn detect_x11_when_display_set() {
        assert_eq!(detect_in(&[("DISPLAY", ":0")]), Backend::X11);
        // An empty WAYLAND_DISPLAY does not count as a Wayland session.
        assert_eq!(
            detect_in(&[("DISPLAY", ":1"), ("WAYLAND_DISPLAY", "")]),
            Backend::X11
        );
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn detect_refuses_wayland_session() {
        let wayland = Backend::Unsupported("Wayland sessions are not supported yet");
        assert_eq!(detect_in(&[("WAYLAND_DISPLAY", "wayland-0")]), wayland);
        assert_eq!(
            detect_in(&[("WAYLAND_DISPLAY", "wayland-0"), ("DISPLAY", ":0")]),
            wayland
        );
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn detect_refuses_without_display() {
        let none = Backend::Unsupported("no X display: DISPLAY is not set");
        assert_eq!(detect_in(&[]), none);
        assert_eq!(detect_in(&[("DISPLAY", "")]), none);
    }

    #[cfg(not(target_os = "linux"))]
    #[test]
    fn detect_is_unsupported() {
        let unsupported = Backend::Unsupported("session mode needs Linux with X11");
        assert_eq!(detect_in(&[]), unsupported);
        assert_eq!(detect_in(&[("DISPLAY", ":0")]), unsupported);
        assert_eq!(detect_in(&[("WAYLAND_DISPLAY", "wayland-0")]), unsupported);
    }
}
