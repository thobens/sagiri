//! User-defined terms: literal, case-insensitive, whole-word.

use std::ops::Range;

use regex::{Regex, RegexBuilder};

use crate::config::ConfigError;
use crate::rule::{Context, Kind, Rule};

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
    }
    Ok(())
}

/// Replaces the user's terms.
pub struct TermRule {
    patterns: Vec<Regex>,
}

impl TermRule {
    /// Builds the rule from `Config::terms`.
    pub fn new(terms: &[String]) -> Result<Self, ConfigError> {
        validate_terms(terms)?;
        let patterns = terms
            .iter()
            .map(|t| {
                RegexBuilder::new(&regex::escape(t.trim()))
                    .case_insensitive(true)
                    .build()
                    .expect("an escaped literal is a valid regex")
            })
            .collect();
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
}
