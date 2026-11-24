//! The seam to the display: the backend watches the selections and sends
//! `Event::Changed` and `Event::BackendLost`.

use std::sync::mpsc::Sender;

use sagiri_core::config::SessionConfig;

use super::core::Event;

/// Starts the display backend, which sends its events on `events`.
///
/// No backend exists yet, so this always fails.
pub fn start(_events: Sender<Event>, _settings: &SessionConfig) -> anyhow::Result<()> {
    anyhow::bail!("session mode has no display backend on this platform")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::mpsc;

    #[test]
    fn no_backend_yet() {
        let err = start(mpsc::channel().0, &SessionConfig::default()).unwrap_err();
        assert_eq!(
            err.to_string(),
            "session mode has no display backend on this platform"
        );
    }
}
