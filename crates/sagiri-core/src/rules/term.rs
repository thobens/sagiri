//! User-defined terms: literal, case-insensitive, whole-word.

use std::ops::Range;

use regex::{Regex, RegexBuilder};

use crate::config::ConfigError;
use crate::rule::{Context, Kind, Rule};

/// The most characters a term may have after trimming.
const MAX_TERM_CHARS: usize = 1000;

/// Checks the terms of the config. An invalid term is an error naming `terms[index]`.
pub(crate) fn validate_terms(terms: &[String]) -> Result<(), ConfigError> {
    for (i, t) in terms.iter().enumerate() {
        let bad = |reason| ConfigError::invalid(format!("terms[{i}]"), reason);
        if t.contains(['\n', '\r']) {
            return Err(bad("term contains a line break"));
        }
        if t.trim().chars().count() < 2 {
            return Err(bad("term is shorter than two characters"));
        }
        if t.trim().chars().count() > MAX_TERM_CHARS {
            return Err(bad("term is longer than 1000 characters"));
        }
    }
    Ok(())
}

/// Replaces the user's terms.
pub struct TermRule {
    patterns: Vec<Regex>,
}

impl TermRule {
    /// Builds the rule from `Config::terms`.
    ///
    /// Terms are trimmed and matched literally, ignoring case, as whole words: a match
    /// must not have a letter or digit directly before or after it. The canonical form is
    /// the lowercase term, so `Zorblax` and `ZORBLAX` get the same replacement, each in its
    /// own case. A replacement has the same length and layout as the term.
    ///
    /// # Errors
    ///
    /// [`ConfigError::Invalid`] with key `terms[index]` and no path, for a term that
    /// contains a line break, has fewer than two or more than 1000 characters after
    /// trimming, or cannot be compiled into a pattern.
    ///
    /// ```
    /// use sagiri_core::engine::Engine;
    /// use sagiri_core::rules::term::TermRule;
    /// # use sagiri_core::{key::Key, pseudo::Pseudonymizer};
    /// # let key = Key::from_bytes(std::array::from_fn(|i| i as u8));
    ///
    /// let rule = TermRule::new(&["Zorblax".to_string()])?;
    /// let engine = Engine::new(Pseudonymizer::new(&key), vec![Box::new(rule)]);
    ///
    /// let lower = engine.obfuscate("zorblax");
    /// assert_ne!(lower, "zorblax");
    /// assert_eq!(lower.len(), 7);
    /// assert_eq!(engine.obfuscate("ZORBLAX"), lower.to_uppercase());
    /// // Not a whole word: kept.
    /// assert_eq!(engine.obfuscate("zorblaxes"), "zorblaxes");
    ///
    /// assert!(TermRule::new(&["x".to_string()]).is_err());
    /// # Ok::<(), Box<dyn std::error::Error>>(())
    /// ```
    pub fn new(terms: &[String]) -> Result<Self, ConfigError> {
        validate_terms(terms)?;
        let patterns = terms
            .iter()
            .enumerate()
            .map(|(i, t)| {
                RegexBuilder::new(&regex::escape(t.trim()))
                    .case_insensitive(true)
                    .build()
                    .map_err(|_| {
                        ConfigError::invalid(format!("terms[{i}]"), "term cannot be compiled")
                    })
            })
            .collect::<Result<_, _>>()?;
        Ok(TermRule { patterns })
    }
}

impl Rule for TermRule {
    fn kind(&self) -> Kind {
        Kind::Term
    }

    fn detect(&self, text: &str) -> Vec<Range<usize>> {
        let mut out = Vec::new();
        for re in &self.patterns {
            for m in re.find_iter(text) {
                let before = text[..m.start()].chars().next_back();
                let after = text[m.end()..].chars().next();
                if before.is_some_and(char::is_alphanumeric)
                    || after.is_some_and(char::is_alphanumeric)
                {
                    continue;
                }
                out.push(m.range());
            }
        }
        out
    }

    fn replace(&self, original: &str, ctx: &Context<'_>) -> String {
        let seed = original.to_lowercase();
        ctx.pseudo().mimic("term", seed.as_bytes(), original)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::Engine;
    use crate::key::Key;
    use crate::pseudo::Pseudonymizer;

    fn engine(terms: &[&str]) -> Engine {
        let mut b = [0u8; 32];
        for (i, x) in b.iter_mut().enumerate() {
            *x = i as u8;
        }
        let terms: Vec<String> = terms.iter().map(|s| s.to_string()).collect();
        Engine::new(
            Pseudonymizer::new(&Key::from_bytes(b)),
            vec![Box::new(TermRule::new(&terms).unwrap())],
        )
    }

    #[test]
    fn case_variants_share_letters() {
        let e = engine(&["Acme Corp"]);
        let a = e.obfuscate("Acme Corp");
        let b = e.obfuscate("acme corp");
        let c = e.obfuscate("ACME CORP");
        assert_ne!(a, "Acme Corp");
        assert_eq!(a.to_lowercase(), b);
        assert_eq!(a.to_uppercase(), c);
        let words: Vec<&str> = a.split(' ').collect();
        assert!(words.iter().all(|w| w.len() == 4));
        assert!(
            words
                .iter()
                .all(|w| w.chars().next().unwrap().is_uppercase())
        );
    }

    #[test]
    fn whole_word_only() {
        let e = engine(&["Acme Corp"]);
        assert_eq!(e.obfuscate("AcmeCorp"), "AcmeCorp");
        assert_eq!(e.obfuscate("Acme Corporation"), "Acme Corporation");
        assert_eq!(e.obfuscate("xAcme Corp"), "xAcme Corp");
        assert_ne!(e.obfuscate("at (Acme Corp)."), "at (Acme Corp).");
    }

    #[test]
    fn longest_overlapping_term_wins() {
        let e = engine(&["Acme", "Acme Corp"]);
        assert_eq!(e.analyze("Acme Corp").len(), 1);
        assert_eq!(e.analyze("Acme Corp")[0].original, "Acme Corp");
        assert_eq!(e.analyze("Acme Corporation").len(), 1);
    }

    #[test]
    fn terms_are_validated() {
        let bad = |t: &str| TermRule::new(&["fine".to_string(), t.to_string()]).map(|_| ());
        for t in ["", " a ", "x", "a\nb"] {
            let e = bad(t).unwrap_err();
            assert!(e.to_string().contains("terms[1]"), "{t:?}: {e}");
        }
    }

    #[test]
    fn overlong_term_is_error() {
        let long = "a".repeat(1001);
        for (terms, index) in [
            (vec![long.clone()], "terms[0]"),
            (vec!["fine".to_string(), long.clone()], "terms[1]"),
        ] {
            let e = TermRule::new(&terms).map(|_| ()).unwrap_err();
            assert!(matches!(e, ConfigError::Invalid { .. }), "{e}");
            let msg = e.to_string();
            assert!(msg.contains(index), "{msg}");
            assert!(msg.contains("term is longer than 1000 characters"), "{msg}");
            assert!(!msg.contains("aaaa"), "{msg}");
        }
    }

    #[test]
    fn term_at_the_limit_is_accepted() {
        for c in ["a", "ä", "1"] {
            let term = c.repeat(1000);
            let rule = TermRule::new(std::slice::from_ref(&term)).unwrap();
            let text = format!("see {term} here");
            assert_eq!(rule.detect(&text), vec![4..4 + term.len()], "{c:?}");
        }
    }

    #[test]
    fn overlong_term_is_measured_after_trimming() {
        let term = format!("  {}  ", "a".repeat(1000));
        assert!(TermRule::new(&[term]).is_ok());
    }

    #[test]
    fn huge_term_does_not_panic() {
        assert!(TermRule::new(&["a".repeat(200_000)]).is_err());
    }

    #[test]
    fn length_reason_matches_the_limit() {
        let e = TermRule::new(&["a".repeat(MAX_TERM_CHARS + 1)])
            .map(|_| ())
            .unwrap_err();
        let reason = format!("term is longer than {MAX_TERM_CHARS} characters");
        assert!(e.to_string().contains(&reason), "{e}");
        assert!(TermRule::new(&["a".repeat(MAX_TERM_CHARS)]).is_ok());
    }
}
