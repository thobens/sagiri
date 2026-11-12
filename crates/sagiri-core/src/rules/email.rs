//! Email address rule.

use std::ops::Range;
use std::sync::LazyLock;

use regex::Regex;

use super::hostname;
use crate::rule::{Context, Kind, Rule};

const LABEL: &str = r"[A-Za-z0-9](?:[A-Za-z0-9-]*[A-Za-z0-9])?";

/// A domain with at least two labels, anchored at the start of the haystack.
static DOMAIN: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(&format!(r"^{LABEL}(?:\.{LABEL})+")).expect("regex"));

fn is_local_char(c: char) -> bool {
    c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '%' | '+' | '-')
}

/// Start of the longest valid local part that ends at `end`, as a byte offset into `text`.
/// `None` if there is none.
fn local_start(text: &str, end: usize) -> Option<usize> {
    let run_start = text[..end]
        .char_indices()
        .rev()
        .take_while(|&(_, c)| is_local_char(c))
        .last()
        .map_or(end, |(i, _)| i);
    let run = &text[run_start..end];
    // Everything up to the last `..` is out, then leading dots.
    let skip = run.rfind("..").map_or(0, |i| i + 2);
    let tail = run[skip..].trim_start_matches('.');
    if tail.is_empty() || tail.ends_with('.') || tail.len() > 64 {
        return None;
    }
    let start = end - tail.len();
    // A run that starts inside a word (`müller@`) is not an address.
    if start == run_start
        && text[..start]
            .chars()
            .next_back()
            .is_some_and(char::is_alphanumeric)
    {
        return None;
    }
    Some(start)
}

/// Detects and replaces email addresses.
pub struct EmailRule;

impl Rule for EmailRule {
    fn kind(&self) -> Kind {
        Kind::Email
    }

    fn detect(&self, text: &str) -> Vec<Range<usize>> {
        let mut out = Vec::new();
        let mut last_end = 0;
        for (at, _) in text.match_indices('@') {
            let Some(start) = local_start(text, at) else {
                continue;
            };
            if start < last_end {
                continue;
            }
            let Some(m) = DOMAIN.find(&text[at + 1..]) else {
                continue;
            };
            let domain = m.as_str();
            let end = at + 1 + domain.len();
            let last = domain.rsplit('.').next().unwrap_or(domain);
            if domain.len() > 253
                || domain.split('.').any(|l| l.len() > 63)
                || last.len() < 2
                || !last.bytes().all(|b| b.is_ascii_alphabetic())
                || !hostname::has_known_suffix(domain)
            {
                continue;
            }
            let mut after = text[end..].chars();
            match after.next() {
                Some(c) if c.is_alphanumeric() || c == '_' || c == '-' => continue,
                Some('.') if after.next().is_some_and(char::is_alphanumeric) => continue,
                _ => {}
            }
            out.push(start..end);
            last_end = end;
        }
        out
    }

    fn replace(&self, original: &str, ctx: &Context<'_>) -> String {
        let Some((local, domain)) = original.rsplit_once('@') else {
            return original.to_string();
        };
        let seed = original.to_ascii_lowercase();
        let mut out = ctx.pseudo().mimic("email-local", seed.as_bytes(), local);
        out.push('@');
        out.push_str(&hostname::pseudonymize(domain, ctx));
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::Engine;
    use crate::key::Key;
    use crate::pseudo::Pseudonymizer;

    fn engine() -> Engine {
        let mut b = [0u8; 32];
        for (i, x) in b.iter_mut().enumerate() {
            *x = i as u8;
        }
        Engine::new(
            Pseudonymizer::new(&Key::from_bytes(b)),
            vec![Box::new(EmailRule)],
        )
    }

    fn found(text: &str) -> Vec<&str> {
        EmailRule
            .detect(text)
            .into_iter()
            .map(|r| &text[r])
            .collect()
    }

    fn map(addr: &str) -> String {
        let e = engine();
        EmailRule.replace(addr, &e.context())
    }

    #[test]
    fn detects_address_only() {
        let cases = [
            ("<john@acme.com>", "john@acme.com"),
            ("mailto:john@acme.com", "john@acme.com"),
            ("\"john@acme.com\"", "john@acme.com"),
            ("Contact john@acme.com.", "john@acme.com"),
            ("john@acme.com,", "john@acme.com"),
            ("git@github.com:org/repo.git", "git@github.com"),
            (".john@acme.com", "john@acme.com"),
            ("john..doe@acme.com", "doe@acme.com"),
        ];
        for (text, want) in cases {
            assert_eq!(found(text), [want], "{text}");
        }
        assert_eq!(found("a@b.com x@y.org"), ["a@b.com", "x@y.org"]);
    }

    #[test]
    fn rejects() {
        for text in [
            "@mention",
            "foo@bar",
            "a@b.c",
            "icon@2x.png",
            "user@localhost",
            "john.@acme.com",
            "john@acme.com_x",
            "john@acme.com-x",
            "john@acme.comx1",
            "user@[10.0.0.1]",
            "éjohn@acme.com",
            "müller@acme.com",
            "john@acme.com.é",
        ] {
            assert!(found(text).is_empty(), "{text}: {:?}", found(text));
        }
        let long = format!("{}@acme.com", "a".repeat(65));
        assert!(found(&long).is_empty());
        let ok = format!("{}@acme.com", "a".repeat(64));
        assert_eq!(found(&ok).len(), 1);
    }

    #[test]
    fn domain_matches_bare_hostname() {
        let e = engine();
        let ctx = e.context();
        let out = map("john.doe@acme.com");
        let (local, domain) = out.split_once('@').unwrap();
        assert_eq!(domain, hostname::pseudonymize("acme.com", &ctx));
        assert_eq!(local.len(), 8);
        assert_eq!(local.as_bytes()[4], b'.');
        assert_ne!(local, "john.doe");
    }

    #[test]
    fn deterministic_and_case_insensitive() {
        assert_eq!(map("john.doe@acme.com"), map("john.doe@acme.com"));
        let lower = map("john.doe@acme.com");
        let mixed = map("John.Doe@ACME.com");
        assert_eq!(mixed.to_ascii_lowercase(), lower.to_ascii_lowercase());
        let recased: String = "John.Doe@ACME.com"
            .chars()
            .zip(lower.chars())
            .map(|(o, l)| {
                if o.is_ascii_uppercase() {
                    l.to_ascii_uppercase()
                } else {
                    l
                }
            })
            .collect();
        assert_eq!(mixed, recased);
    }

    #[test]
    fn separators_and_suffix_kept() {
        let out = map("first_last+tag-1@mail.acme.co.uk");
        let (local, domain) = out.split_once('@').unwrap();
        let b = local.as_bytes();
        assert_eq!(b.len(), "first_last+tag-1".len());
        assert_eq!((b[5], b[10], b[14]), (b'_', b'+', b'-'));
        assert!(b[15].is_ascii_digit());
        assert!(domain.ends_with(".co.uk"));
    }

    #[test]
    fn percent_stays_in_place() {
        let out = map("a%b.c%d@acme.com");
        let local = out.split_once('@').unwrap().0;
        assert_eq!(local.len(), "a%b.c%d".len());
        assert_eq!((local.as_bytes()[1], local.as_bytes()[3]), (b'%', b'.'));
        assert_eq!(local.as_bytes()[5], b'%');
        assert_ne!(local, "a%b.c%d");
    }

    #[test]
    fn reserved_domain_kept_local_replaced() {
        let out = map("jane@example.com");
        assert!(out.ends_with("@example.com"));
        assert!(!out.starts_with("jane@"));
    }
}
