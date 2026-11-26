//! Detection and pseudonymization for Sagiri.
//!
//! The crate finds sensitive values in a text (IP addresses, URLs, hostnames, email
//! addresses, payment card numbers, IBANs, phone numbers and user-defined terms) and
//! replaces each with a realistic-looking fake value derived from a secret key. The same
//! key and the same original always give the same replacement, in every future version of
//! this crate. Nothing about the originals is stored, and replacements cannot be reversed
//! without the key. Whoever has the key can recompute them, and for small value spaces such
//! as IPv4 addresses can find the original by trying every value.
//!
//! This crate holds no terminal I/O. It never writes pasted text anywhere.
//!
//! # Modules
//!
//! - [`key`]: the 32-byte secret [`Key`](key::Key) and its key file.
//! - [`pseudo`]: the [`Pseudonymizer`](pseudo::Pseudonymizer), the keyed source of every
//!   derived value.
//! - [`rule`]: the [`Rule`](rule::Rule) trait, the [`Kind`](rule::Kind) of a value and the
//!   [`Context`](rule::Context) a rule replaces in.
//! - [`engine`]: the [`Engine`](engine::Engine) that runs rules over a text, and
//!   [`render`](engine::render) for partial replacement.
//! - [`rules`]: the built-in rules, collected by [`default_rules`](rules::default_rules).
//! - [`config`]: the config file with allowlist, terms, rule toggles and session settings.
//! - [`paths`]: the XDG directories where key and config live.
//!
//! # Quick start
//!
//! ```
//! use sagiri_core::engine::Engine;
//! use sagiri_core::key::Key;
//! use sagiri_core::pseudo::Pseudonymizer;
//! use sagiri_core::rules::default_rules;
//!
//! // The test key 0x00, 0x01, … 0x1f. Real programs load one with `Key::load_or_create`.
//! let key = Key::from_bytes(std::array::from_fn(|i| i as u8));
//! let engine = Engine::new(Pseudonymizer::new(&key), default_rules());
//!
//! let out = engine.obfuscate("card 4111 1111 1111 1111, host 192.0.2.10");
//! assert_eq!(&out[..24], "card 4304 2320 3316 5444");
//! assert!(!out.contains("192.0.2.10"));
//! // The same key and input always give the same output.
//! assert_eq!(engine.obfuscate("card 4111 1111 1111 1111, host 192.0.2.10"), out);
//! ```

#![deny(clippy::print_stdout, clippy::print_stderr)]
#![deny(missing_docs)]

pub mod config;
pub mod engine;
pub mod key;
pub mod paths;
pub mod pseudo;
pub mod rule;
pub mod rules;
