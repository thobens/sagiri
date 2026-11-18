//! Detection and pseudonymization for Sagiri.
//!
//! This crate holds no terminal I/O. It never writes pasted text anywhere.

#![deny(clippy::print_stdout, clippy::print_stderr)]

pub mod config;
pub mod engine;
pub mod key;
pub mod paths;
pub mod pseudo;
pub mod rule;
pub mod rules;
