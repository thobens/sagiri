//! Runs rules over a text, resolves overlaps and renders the output.

use std::fmt;
use std::ops::Range;

use crate::pseudo::Pseudonymizer;
use crate::rule::{Context, Kind, Rule};

/// One replaced value found in the input.
#[derive(Clone)]
pub struct Finding {
    /// Byte range in the input text.
    pub span: Range<usize>,
    pub kind: Kind,
    pub original: String,
    pub replacement: String,
}

/// Output text with the position of every finding in it.
#[derive(Clone)]
pub struct Rendered {
    pub text: String,
    pub spans: Vec<RenderedSpan>,
}

#[derive(Clone, Debug)]
pub struct RenderedSpan {
    /// Byte range in `Rendered::text`.
    pub range: Range<usize>,
    /// Index into the findings slice.
    pub finding: usize,
    /// False if the original was kept.
    pub replaced: bool,
}

// Debug output never includes pasted text.
impl fmt::Debug for Finding {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Finding")
            .field("span", &self.span)
            .field("kind", &self.kind)
            .finish_non_exhaustive()
    }
}

impl fmt::Debug for Rendered {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Rendered")
            .field("spans", &self.spans)
            .finish_non_exhaustive()
    }
}

/// Runs a set of rules with one pseudonymizer.
pub struct Engine {
    pseudo: Pseudonymizer,
    rules: Vec<Box<dyn Rule>>,
}

struct Candidate {
    range: Range<usize>,
    rule: usize,
}

impl Engine {
    pub fn new(pseudo: Pseudonymizer, rules: Vec<Box<dyn Rule>>) -> Self {
        Engine { pseudo, rules }
    }

    pub fn context(&self) -> Context<'_> {
        Context {
            engine: self,
            depth: 0,
        }
    }

    pub(crate) fn pseudo(&self) -> &Pseudonymizer {
        &self.pseudo
    }

    /// Finds all values to replace, sorted by start, without overlaps.
    pub fn analyze(&self, text: &str) -> Vec<Finding> {
        self.analyze_at(text, 0)
    }

    /// Replaces every finding.
    pub fn obfuscate(&self, text: &str) -> String {
        self.obfuscate_at(text, 0)
    }

    pub(crate) fn obfuscate_at(&self, text: &str, depth: u8) -> String {
        let findings = self.analyze_at(text, depth);
        render(text, &findings, |_| true).text
    }

    fn analyze_at(&self, text: &str, depth: u8) -> Vec<Finding> {
        let mut candidates = Vec::new();
        for (rule, r) in self.rules.iter().enumerate() {
            for range in r.detect(text) {
                if valid_range(text, &range) {
                    candidates.push(Candidate { range, rule });
                }
            }
        }
        candidates.sort_by(|a, b| {
            a.range
                .start
                .cmp(&b.range.start)
                .then(b.range.len().cmp(&a.range.len()))
                .then(a.rule.cmp(&b.rule))
        });

        let ctx = Context {
            engine: self,
            depth,
        };
        let mut findings: Vec<Finding> = Vec::new();
        let mut accepted_end = 0usize;
        for c in candidates {
            // Sorted by start, so only the latest accepted range can overlap.
            if c.range.start < accepted_end {
                continue;
            }
            accepted_end = c.range.end;
            let rule = &self.rules[c.rule];
            let original = &text[c.range.clone()];
            let replacement = rule.replace(original, &ctx);
            if replacement == original || replacement.contains(['\n', '\r']) {
                continue;
            }
            findings.push(Finding {
                span: c.range,
                kind: rule.kind(),
                original: original.to_string(),
                replacement,
            });
        }
        findings
    }
}

fn valid_range(text: &str, r: &Range<usize>) -> bool {
    r.start < r.end
        && r.end <= text.len()
        && text.is_char_boundary(r.start)
        && text.is_char_boundary(r.end)
        && !text[r.clone()].contains(['\n', '\r'])
}

/// Builds the output text. Findings for which `include` is false keep their original.
///
/// `findings` must come from [`Engine::analyze`] on this same `text`, unmodified. Findings
/// that are unsorted, overlapping or out of range for `text` make this function panic.
pub fn render(text: &str, findings: &[Finding], include: impl Fn(&Finding) -> bool) -> Rendered {
    let mut out = String::with_capacity(text.len());
    let mut spans = Vec::with_capacity(findings.len());
    let mut pos = 0usize;
    for (i, f) in findings.iter().enumerate() {
        out.push_str(&text[pos..f.span.start]);
        let replaced = include(f);
        let start = out.len();
        out.push_str(if replaced {
            &f.replacement
        } else {
            &f.original
        });
        spans.push(RenderedSpan {
            range: start..out.len(),
            finding: i,
            replaced,
        });
        pos = f.span.end;
    }
    out.push_str(&text[pos..]);
    Rendered { text: out, spans }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::key::Key;
    use proptest::prelude::*;

    /// Matches every occurrence of a literal; replaces with a fixed string.
    struct Lit {
        kind: Kind,
        needle: &'static str,
        with: &'static str,
    }

    impl Rule for Lit {
        fn kind(&self) -> Kind {
            self.kind
        }
        fn detect(&self, text: &str) -> Vec<Range<usize>> {
            text.match_indices(self.needle)
                .map(|(i, m)| i..i + m.len())
                .collect()
        }
        fn replace(&self, _: &str, _: &Context<'_>) -> String {
            self.with.to_string()
        }
    }

    /// Returns fixed ranges, whatever the text.
    struct Fixed(Kind, Vec<Range<usize>>, &'static str);

    impl Rule for Fixed {
        fn kind(&self) -> Kind {
            self.0
        }
        fn detect(&self, _: &str) -> Vec<Range<usize>> {
            self.1.clone()
        }
        fn replace(&self, _: &str, _: &Context<'_>) -> String {
            self.2.to_string()
        }
    }

    fn lit(kind: Kind, needle: &'static str, with: &'static str) -> Box<dyn Rule> {
        Box::new(Lit { kind, needle, with })
    }

    fn engine(rules: Vec<Box<dyn Rule>>) -> Engine {
        let key = Key::from_bytes(std::array::from_fn(|i| i as u8));
        Engine::new(Pseudonymizer::new(&key), rules)
    }

    #[test]
    fn no_rules_returns_input() {
        let e = engine(vec![]);
        assert_eq!(e.obfuscate(""), "");
        assert_eq!(e.obfuscate("hello\nworld"), "hello\nworld");
    }

    #[test]
    fn rules_matching_nothing_return_input() {
        let e = engine(vec![lit(Kind::Term, "zzz", "X")]);
        assert_eq!(e.obfuscate(""), "");
        assert_eq!(e.obfuscate("abc"), "abc");
    }

    #[test]
    fn overlap_identical_ranges_earlier_rule_wins() {
        let e = engine(vec![
            lit(Kind::Term, "foo", "A"),
            lit(Kind::Url, "foo", "B"),
        ]);
        let f = e.analyze("foo");
        assert_eq!(f.len(), 1);
        assert_eq!(f[0].replacement, "A");
        assert_eq!(f[0].kind, Kind::Term);
    }

    #[test]
    fn overlap_inner_range_outer_wins() {
        // The inner rule comes first in rule order, yet the longer match wins.
        let e = engine(vec![
            lit(Kind::Term, "bar", "I"),
            lit(Kind::Url, "foobarbaz", "O"),
        ]);
        assert_eq!(e.obfuscate("xfoobarbazx"), "xOx");
        // Same start, longer wins too.
        let e = engine(vec![
            lit(Kind::Term, "foo", "I"),
            lit(Kind::Url, "foobar", "O"),
        ]);
        assert_eq!(e.obfuscate("foobar"), "O");
    }

    #[test]
    fn overlap_partial_earlier_start_wins() {
        let e = engine(vec![
            lit(Kind::Term, "cdef", "L"),
            lit(Kind::Url, "abcd", "E"),
        ]);
        assert_eq!(e.obfuscate("abcdef"), "Eef");
    }

    #[test]
    fn adjacent_ranges_both_kept() {
        let e = engine(vec![lit(Kind::Term, "ab", "1"), lit(Kind::Url, "cd", "2")]);
        let f = e.analyze("abcd");
        assert_eq!(f.len(), 2);
        assert_eq!(e.obfuscate("abcd"), "12");
    }

    #[test]
    fn invalid_ranges_are_ignored() {
        let text = "a\u{e9}b\nc";
        // \u{e9} occupies bytes 1..3; the newline is byte 4.
        #[allow(clippy::reversed_empty_ranges)]
        let bad = vec![2..2, 3..2, 0..99, 2..3, 1..2, 0..text.len(), 4..6, 5..99];
        let e = engine(vec![Box::new(Fixed(Kind::Term, bad, "X"))]);
        assert!(e.analyze(text).is_empty());
        assert_eq!(e.obfuscate(text), text);
    }

    #[test]
    fn debug_does_not_expose_text() {
        let e = engine(vec![lit(Kind::Term, "secret", "other")]);
        let findings = e.analyze("a secret b");
        let r = render("a secret b", &findings, |_| true);
        let shown = format!("{findings:?} {r:?}");
        assert!(!shown.contains("secret") && !shown.contains("other"));
    }

    #[test]
    fn noop_replacement_is_not_a_finding() {
        let e = engine(vec![lit(Kind::Term, "foo", "foo")]);
        assert!(e.analyze("foo foo").is_empty());
    }

    #[test]
    fn replacement_with_newline_is_not_applied() {
        for bad in ["a\nb", "a\rb"] {
            let e = engine(vec![lit(Kind::Term, "foo", bad)]);
            assert!(e.analyze("foo").is_empty());
            assert_eq!(e.obfuscate("x foo"), "x foo");
        }
    }

    #[test]
    fn rendered_spans_match_findings() {
        let e = engine(vec![lit(Kind::Term, "foo", "quux!")]);
        let text = "a foo b foo\u{e9} foo";
        let findings = e.analyze(text);
        assert_eq!(findings.len(), 3);
        for pred in [
            (|_: &Finding| true) as fn(&Finding) -> bool,
            |f| f.span.start != 2,
            |_| false,
        ] {
            let r = render(text, &findings, pred);
            assert_eq!(r.spans.len(), findings.len());
            for (i, s) in r.spans.iter().enumerate() {
                assert_eq!(s.finding, i);
                let f = &findings[s.finding];
                assert_eq!(s.replaced, pred(f));
                let want = if s.replaced {
                    &f.replacement
                } else {
                    &f.original
                };
                assert_eq!(&r.text[s.range.clone()], want);
            }
        }
    }

    #[test]
    fn render_include_none_returns_input() {
        let e = engine(vec![lit(Kind::Term, "foo", "quux!")]);
        let text = "foo\nbar foo";
        let findings = e.analyze(text);
        assert_eq!(render(text, &findings, |_| false).text, text);
    }

    /// Matches `[foo]` and replaces the part between the brackets via `nested`.
    struct Outer;

    impl Rule for Outer {
        fn kind(&self) -> Kind {
            Kind::Url
        }
        fn detect(&self, text: &str) -> Vec<Range<usize>> {
            text.match_indices("[foo]")
                .map(|(i, m)| i..i + m.len())
                .collect()
        }
        fn replace(&self, original: &str, ctx: &Context<'_>) -> String {
            format!("<{}>", ctx.nested(&original[1..original.len() - 1]))
        }
    }

    #[test]
    fn nested_uses_inner_rule() {
        let e = engine(vec![Box::new(Outer), lit(Kind::Term, "foo", "BAR")]);
        assert_eq!(e.obfuscate("x [foo] y"), "x <BAR> y");
    }

    struct Whole;

    impl Rule for Whole {
        fn kind(&self) -> Kind {
            Kind::Term
        }
        fn detect(&self, text: &str) -> Vec<Range<usize>> {
            text.match_indices("foo")
                .map(|(i, m)| i..i + m.len())
                .collect()
        }
        fn replace(&self, original: &str, ctx: &Context<'_>) -> String {
            format!("{}!", ctx.nested(original))
        }
    }

    #[test]
    fn nested_on_whole_match_terminates() {
        let e = engine(vec![Box::new(Whole)]);
        // The contexts at depths 0 to 8 each append one "!". The `nested` call
        // made from depth 8 returns its fragment unchanged, which ends the recursion.
        assert_eq!(e.obfuscate("foo"), format!("foo{}", "!".repeat(9)));
    }

    proptest! {
        #[test]
        fn output_without_findings_equals_input_without_matches(
            text in "(\\PC|\n|\r|a|b\u{e9}){0,60}"
        ) {
            let e = engine(vec![lit(Kind::Term, "a", "ZZ"), lit(Kind::Url, "b\u{e9}", "Y")]);
            let findings = e.analyze(&text);
            let r = render(&text, &findings, |_| true);
            prop_assert_eq!(&r.text, &e.obfuscate(&text));

            // The needles cannot overlap, so removing them in turn is independent
            // of the engine.
            let kept_in = text.replace("b\u{e9}", "").replace('a', "");

            let mut kept_out = String::new();
            let mut pos = 0;
            for s in &r.spans {
                kept_out.push_str(&r.text[pos..s.range.start]);
                pos = s.range.end;
            }
            kept_out.push_str(&r.text[pos..]);

            prop_assert_eq!(kept_in, kept_out);
            prop_assert_eq!(text.lines().count(), r.text.lines().count());
            prop_assert_eq!(text.matches('\n').count(), r.text.matches('\n').count());
            prop_assert_eq!(text.matches('\r').count(), r.text.matches('\r').count());
        }
    }
}
