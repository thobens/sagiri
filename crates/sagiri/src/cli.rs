//! `sagiri-cli`: obfuscates text from a selectable input and writes it to a selectable
//! output.

use std::fmt;
use std::io::{Read, Write};

use anyhow::Context as _;
use sagiri_core::engine::Engine;

use crate::clipboard::{Content, Selection, SelectionClipboard};
use crate::filter;

/// Where the input comes from.
#[derive(Clone, Copy, PartialEq, Eq, Debug, clap::ValueEnum)]
pub enum Source {
    /// Standard input, read to its end.
    Stdin,
    /// The regular clipboard.
    Clipboard,
    /// The PRIMARY selection.
    Primary,
}

/// Where the output goes.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default, clap::ValueEnum)]
pub enum Sink {
    /// Standard output.
    #[default]
    Stdout,
    /// The regular clipboard.
    Clipboard,
    /// The PRIMARY selection.
    Primary,
}

/// Writes text to a selection. The binary passes the holder or a direct write.
pub type SelectionWriter<'a> = dyn FnMut(Selection, &str) -> anyhow::Result<()> + 'a;

/// Writes `text` to `selection` directly, for platforms where the system keeps the content.
pub fn write_direct(
    selections: &mut dyn SelectionClipboard,
    selection: Selection,
    text: &str,
) -> anyhow::Result<()> {
    selections
        .write_text(selection, text)
        .with_context(|| format!("cannot write {}", selection_name(selection)))
}

fn selection_name(selection: Selection) -> &'static str {
    match selection {
        Selection::Clipboard => "the clipboard",
        Selection::Primary => "the primary selection",
    }
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
    let selection = match source {
        Source::Stdin => return filter::read_text(stdin),
        Source::Clipboard => Selection::Clipboard,
        Source::Primary => Selection::Primary,
    };
    let name = selection_name(selection);
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

/// What a run reads, where it writes and whether it trims the output.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Options {
    pub source: Source,
    pub sink: Sink,
    /// Remove one trailing line break from the output (`-n`).
    pub trim: bool,
}

/// Reads the input, obfuscates it and writes the result to the sink: `output` for stdout,
/// `write_selection` for a selection, in which case nothing goes to `output`. On an error
/// nothing is written. A broken pipe on `output` is not an error.
pub fn run(
    engine: &Engine,
    options: Options,
    stdin: impl Read,
    selections: &mut dyn SelectionClipboard,
    write_selection: &mut SelectionWriter<'_>,
    output: impl Write,
) -> anyhow::Result<()> {
    let input = read_input(options.source, stdin, selections)?;
    let result = engine.obfuscate(&input);
    let text = if options.trim {
        trim_newline(&result)
    } else {
        &result
    };
    match options.sink {
        Sink::Stdout => filter::write_text(output, text),
        Sink::Clipboard => write_selection(Selection::Clipboard, text),
        Sink::Primary => write_selection(Selection::Primary, text),
    }
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
        let (r, out, written) = run_to(source, Sink::default(), trim, stdin, fake);
        assert!(written.is_empty(), "the selection writer was called");
        (r, out)
    }

    /// Runs against `fake` with `sink`. The selection writer records each call and then
    /// writes to `fake`. Returns the result, what went to stdout and the recorded writes.
    fn run_to(
        source: Source,
        sink: Sink,
        trim: bool,
        stdin: &[u8],
        fake: &FakeSelections,
    ) -> (anyhow::Result<()>, Vec<u8>, Vec<(Selection, String)>) {
        let mut out = Vec::new();
        let mut written = Vec::new();
        let mut target = fake.clone();
        let mut writer = |selection: Selection, text: &str| {
            written.push((selection, text.to_string()));
            write_direct(&mut target, selection, text)
        };
        let options = Options { source, sink, trim };
        let r = run(
            &engine(),
            options,
            stdin,
            &mut fake.clone(),
            &mut writer,
            &mut out,
        );
        (r, out, written)
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

        // A regression guard only: the fake's error is a fixed string, so this part fails
        // only if `read_input` itself starts to put selection content into its message.
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

    #[test]
    fn out_stdout_is_default() {
        assert_eq!(Sink::default(), Sink::Stdout);
        let fake = FakeSelections::default();
        let (r, out, written) = run_to(
            Source::Stdin,
            Sink::default(),
            false,
            TEXT.as_bytes(),
            &fake,
        );
        r.unwrap();
        assert_eq!(String::from_utf8(out).unwrap(), engine().obfuscate(TEXT));
        assert!(written.is_empty());
        for selection in [Selection::Clipboard, Selection::Primary] {
            assert_eq!(fake.writes(selection), 0);
        }
    }

    #[test]
    fn out_selection_writes_obfuscated_text() {
        for (sink, selection, other) in [
            (Sink::Clipboard, Selection::Clipboard, Selection::Primary),
            (Sink::Primary, Selection::Primary, Selection::Clipboard),
        ] {
            let fake = FakeSelections::default();
            let (r, out, written) = run_to(Source::Stdin, sink, false, TEXT.as_bytes(), &fake);
            r.unwrap();
            let expected = engine().obfuscate(TEXT);
            assert_eq!(written, vec![(selection, expected.clone())]);
            assert!(out.is_empty());
            assert_eq!(fake.get(selection), Content::Text(expected));
            assert_eq!(fake.writes(other), 0);
        }
    }

    #[test]
    fn clipboard_to_clipboard_replaces_in_place() {
        let fake = FakeSelections::default();
        fake.set(Selection::Clipboard, Content::Text(TEXT.into()));
        let (r, out, written) = run_to(Source::Clipboard, Sink::Clipboard, false, b"", &fake);
        r.unwrap();
        let expected = engine().obfuscate(TEXT);
        assert_ne!(expected, TEXT);
        assert_eq!(written, vec![(Selection::Clipboard, expected.clone())]);
        assert!(out.is_empty());
        assert_eq!(fake.get(Selection::Clipboard), Content::Text(expected));
        assert_eq!(fake.reads(Selection::Clipboard), 1);
        assert_eq!(fake.writes(Selection::Clipboard), 1);
        assert_eq!(fake.reads(Selection::Primary), 0);
        assert_eq!(fake.writes(Selection::Primary), 0);
    }

    #[test]
    fn trim_newline_applies_to_selection_output() {
        for (input, trim, expected_input) in [
            (format!("{TEXT}\n"), true, TEXT.to_string()),
            (format!("{TEXT}\r\n"), true, TEXT.to_string()),
            (format!("{TEXT}\n\n"), true, format!("{TEXT}\n")),
            (format!("{TEXT}\n"), false, format!("{TEXT}\n")),
        ] {
            let fake = FakeSelections::default();
            let (r, out, written) =
                run_to(Source::Stdin, Sink::Primary, trim, input.as_bytes(), &fake);
            r.unwrap();
            assert!(out.is_empty());
            let expected = engine().obfuscate(&expected_input);
            assert_eq!(written, vec![(Selection::Primary, expected)]);
        }
    }

    #[test]
    fn selection_write_errors_name_the_selection_only() {
        for (sink, selection, prefix) in [
            (
                Sink::Clipboard,
                Selection::Clipboard,
                "cannot write the clipboard: ",
            ),
            (
                Sink::Primary,
                Selection::Primary,
                "cannot write the primary selection: ",
            ),
        ] {
            let fake = FakeSelections::default();
            fake.fail_write(selection, true);
            let (r, out, _) = run_to(Source::Stdin, sink, false, TEXT.as_bytes(), &fake);
            let msg = error_message(r);
            assert!(msg.starts_with(prefix), "{msg}");
            assert!(!msg.contains("172.55.0.1"), "{msg}");
            assert!(out.is_empty());
        }
    }

    #[test]
    fn input_error_writes_no_selection() {
        let fake = FakeSelections::default();
        let (r, out, written) = run_to(Source::Clipboard, Sink::Clipboard, false, b"", &fake);
        assert_eq!(error_message(r), "the clipboard holds no text");
        assert!(out.is_empty());
        assert!(written.is_empty());
    }
}
