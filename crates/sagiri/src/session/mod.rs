//! Session mode: while a session is active, text on the clipboard is replaced by its
//! obfuscated form.

pub mod backend;
pub mod control;
pub mod core;
pub mod daemon;
pub mod hotkey;
pub mod notify;
#[cfg(target_os = "linux")]
pub mod x11;
