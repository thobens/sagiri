//! `sagiri-cli`: obfuscates text from a selectable input and writes it to stdout.

use std::fmt;
use std::io::{Read, Write};

use anyhow::Context as _;
use sagiri_core::engine::Engine;

use crate::clipboard::{Content, Selection, SelectionClipboard};
use crate::filter;

/// Where the input comes from.
#[derive(Clone, Copy, PartialEq, Eq, Debug, clap::ValueEnum)]
pub enum Source {
    Stdin,
    /// The regular clipboard.
    Clipboard,
    /// The PRIMARY selection.
    Primary,
}

/// No input was selected and stdin is a terminal.
#[derive(Debug, PartialEq, Eq)]
pub struct NoInput;

impl fmt::Display for NoInput {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("no input: pipe text into sagiri-cli or use --in clipboard")
    }
}

impl std::error::Error for NoInput {}

/// Picks the input: a flag wins, otherwise stdin unless it is a terminal.
pub fn resolve_input(flag: Option<Source>, stdin_is_terminal: bool) -> Result<Source, NoInput> {
    match flag {
        Some(source) => Ok(source),
        None if stdin_is_terminal => Err(NoInput),
        None => Ok(Source::Stdin),
    }
}

/// Reads the input text from `source`. `stdin` and `selections` are touched only when
/// `source` names them.
pub fn read_input(
    source: Source,
    stdin: impl Read,
    selections: &mut dyn SelectionClipboard,
) -> anyhow::Result<String> {
    let (selection, name) = match source {
        Source::Stdin => return filter::read_text(stdin),
        Source::Clipboard => (Selection::Clipboard, "the clipboard"),
        Source::Primary => (Selection::Primary, "the primary selection"),
    };
    match selections
        .read(selection)
        .with_context(|| format!("cannot read {name}"))?
    {
        Content::Text(text) => Ok(text),
        Content::Files => anyhow::bail!("{name} holds files, not text"),
        Content::Other => anyhow::bail!("{name} holds no text"),
    }
}

/// Removes exactly one trailing `\n` or `\r\n`, if there is one.
pub fn trim_newline(text: &str) -> &str {
    text.strip_suffix("\r\n")
        .or_else(|| text.strip_suffix('\n'))
        .unwrap_or(text)
}

/// Reads the input, obfuscates it and writes the result to `output`. On an error nothing
/// is written. A broken pipe on `output` is not an error.
pub fn run(
    engine: &Engine,
    source: Source,
    trim: bool,
    stdin: impl Read,
    selections: &mut dyn SelectionClipboard,
    output: impl Write,
) -> anyhow::Result<()> {
    let input = read_input(source, stdin, selections)?;
    let result = engine.obfuscate(&input);
    let text = if trim { trim_newline(&result) } else { &result };
    filter::write_text(output, text)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::clipboard::FakeSelections;
    use sagiri_core::key::Key;
    use sagiri_core::pseudo::Pseudonymizer;
    use sagiri_core::rules::default_rules;
    use std::array;

    fn engine() -> Engine {
        let bytes: [u8; 32] = array::from_fn(|i| i as u8);
        Engine::new(Pseudonymizer::new(&Key::from_bytes(bytes)), default_rules())
    }

    /// Runs against `fake` and returns the result and what was written.
    fn run_with(
        source: Source,
        trim: bool,
        stdin: &[u8],
        fake: &FakeSelections,
    ) -> (anyhow::Result<()>, Vec<u8>) {
        let mut out = Vec::new();
        let mut handle = fake.clone();
        let r = run(&engine(), source, trim, stdin, &mut handle, &mut out);
        (r, out)
    }

    fn error_message(r: anyhow::Result<()>) -> String {
        format!("{:#}", r.unwrap_err())
    }

    const TEXT: &str = "server 172.55.0.1 is down";

    #[test]
    fn flag_wins_over_terminal_check() {
        for source in [Source::Stdin, Source::Clipboard, Source::Primary] {
            assert_eq!(resolve_input(Some(source), true), Ok(source));
            assert_eq!(resolve_input(Some(source), false), Ok(source));
        }
    }

    #[test]
    fn stdin_is_default_when_piped() {
        assert_eq!(resolve_input(None, false), Ok(Source::Stdin));
    }

    #[test]
    fn terminal_without_flag_is_no_input() {
        assert_eq!(resolve_input(None, true), Err(NoInput));
        assert_eq!(
            NoInput.to_string(),
            "no input: pipe text into sagiri-cli or use --in clipboard"
        );
    }

    #[test]
    fn clipboard_input_is_obfuscated() {
        let fake = FakeSelections::default();
        fake.set(Selection::Clipboard, Content::Text(TEXT.into()));
        let (r, out) = run_with(Source::Clipboard, false, b"", &fake);
        r.unwrap();
        let out = String::from_utf8(out).unwrap();
        assert_eq!(out, engine().obfuscate(TEXT));
        assert!(!out.contains("172.55.0.1"), "{out}");
    }

    #[test]
    fn primary_input_is_obfuscated() {
        let fake = FakeSelections::default();
        fake.set(Selection::Primary, Content::Text(TEXT.into()));
        fake.set(
            Selection::Clipboard,
            Content::Text("ping 198.51.100.4 failed".into()),
        );
        let (r, out) = run_with(Source::Primary, false, b"", &fake);
        r.unwrap();
        let out = String::from_utf8(out).unwrap();
        assert_eq!(out, engine().obfuscate(TEXT));
        assert!(!out.contains("172.55.0.1"), "{out}");
        assert_eq!(fake.reads(Selection::Clipboard), 0);
    }

    #[test]
    fn selection_without_text_is_error() {
        for (source, msg) in [
            (Source::Clipboard, "the clipboard holds no text"),
            (Source::Primary, "the primary selection holds no text"),
        ] {
            let fake = FakeSelections::default();
            let (r, out) = run_with(source, false, b"", &fake);
            assert_eq!(error_message(r), msg);
            assert!(out.is_empty());
        }
    }

    #[test]
    fn files_on_selection_is_error() {
        for (source, selection, msg) in [
            (
                Source::Clipboard,
                Selection::Clipboard,
                "the clipboard holds files, not text",
            ),
            (
                Source::Primary,
                Selection::Primary,
                "the primary selection holds files, not text",
            ),
        ] {
            let fake = FakeSelections::default();
            fake.set(selection, Content::Files);
            let (r, out) = run_with(source, false, b"", &fake);
            assert_eq!(error_message(r), msg);
            assert!(out.is_empty());
        }
    }

    #[test]
    fn failing_selection_names_the_selection() {
        for (source, selection, prefix) in [
            (
                Source::Clipboard,
                Selection::Clipboard,
                "cannot read the clipboard: ",
            ),
            (
                Source::Primary,
                Selection::Primary,
                "cannot read the primary selection: ",
            ),
        ] {
            let fake = FakeSelections::default();
            fake.fail_read(selection, true);
            let (r, out) = run_with(source, false, b"", &fake);
            let msg = error_message(r);
            assert!(msg.starts_with(prefix), "{msg}");
            assert!(out.is_empty());
        }
    }

    #[test]
    fn error_messages_never_contain_content() {
        let fake = FakeSelections::default();
        let (r, out) = run_with(Source::Stdin, false, b"a\xff\xfeSECRET", &fake);
        let msg = error_message(r);
        assert_eq!(msg, "input is not valid UTF-8");
        assert!(!msg.contains("SECRET"));
        assert!(out.is_empty());

        let fake = FakeSelections::default();
        fake.set(
            Selection::Clipboard,
            Content::Text("SECRET 172.55.0.1".into()),
        );
        fake.fail_read(Selection::Clipboard, true);
        let (r, out) = run_with(Source::Clipboard, false, b"", &fake);
        let msg = error_message(r);
        assert!(!msg.contains("SECRET"), "{msg}");
        assert!(!msg.contains("172.55.0.1"), "{msg}");
        assert!(out.is_empty());
    }

    #[test]
    fn trim_newline_removes_exactly_one() {
        assert_eq!(trim_newline("a\n\n"), "a\n");
        assert_eq!(trim_newline("a\r\n"), "a");
        assert_eq!(trim_newline("a"), "a");
        assert_eq!(trim_newline(""), "");

        let fake = FakeSelections::default();
        fake.set(Selection::Clipboard, Content::Text(format!("{TEXT}\n")));
        let (r, out) = run_with(Source::Clipboard, true, b"", &fake);
        r.unwrap();
        assert_eq!(String::from_utf8(out).unwrap(), engine().obfuscate(TEXT));
    }

    #[test]
    fn stdin_input_is_obfuscated_without_touching_selections() {
        let fake = FakeSelections::default();
        let (r, out) = run_with(Source::Stdin, false, TEXT.as_bytes(), &fake);
        r.unwrap();
        assert_eq!(String::from_utf8(out).unwrap(), engine().obfuscate(TEXT));
        assert_eq!(fake.reads(Selection::Clipboard), 0);
        assert_eq!(fake.reads(Selection::Primary), 0);
    }

    #[test]
    fn input_selection_is_not_written() {
        let fake = FakeSelections::default();
        fake.set(Selection::Clipboard, Content::Text(TEXT.into()));
        let (r, _) = run_with(Source::Clipboard, false, b"", &fake);
        r.unwrap();
        for selection in [Selection::Clipboard, Selection::Primary] {
            assert_eq!(fake.writes(selection), 0);
            assert_eq!(fake.clears(selection), 0);
        }
        assert_eq!(fake.get(Selection::Clipboard), Content::Text(TEXT.into()));
    }
}
