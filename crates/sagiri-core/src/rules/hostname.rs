//! Hostname rule and the hostname mapping shared with the URL rule.

use std::ops::Range;
use std::sync::LazyLock;

use regex::Regex;

use crate::rule::{Context, Kind, Rule};

/// Top-level domains that are also common file extensions or identifiers in code. A bare
/// name ending in one of these is not reported. Keep sorted.
const AMBIGUOUS_TLDS: &[&str] = &[
    "ac", "am", "app", "audio", "auto", "bar", "box", "call", "cc", "cf", "city", "cloud",
    "company", "contact", "date", "dev", "email", "events", "foo", "global", "group", "host", "id",
    "in", "info", "int", "java", "la", "link", "live", "map", "md", "media", "mk", "ml", "mov",
    "name", "network", "new", "next", "now", "one", "online", "open", "page", "phone", "pl", "pm",
    "prod", "ps", "pub", "py", "read", "rs", "run", "search", "services", "sh", "site", "so",
    "space", "store", "stream", "style", "support", "swift", "team", "tf", "tools", "top", "video",
    "work", "works", "world", "zip", "zone",
];

/// Well-known names from code that look like hostnames. Keep sorted, lower case.
const NOT_HOSTNAMES: &[&str] = &[
    "env.local",
    "java.io",
    "java.net",
    "javax.net",
    "kotlin.io",
    "scala.io",
    "std.io",
    "std.net",
    "system.io",
    "system.net",
];

/// Labels that may trail a hostname inside a file name. Keep sorted.
const FILE_EXTENSIONS: &[&str] = &[
    "bak", "bz2", "cer", "cfg", "conf", "crt", "csr", "css", "db", "gz", "htm", "html", "ini",
    "js", "json", "key", "lock", "log", "old", "orig", "p12", "pem", "pfx", "php", "pid", "pub",
    "service", "sock", "sql", "tar", "tgz", "tmp", "toml", "txt", "xml", "xz", "yaml", "yml",
    "zip", "zone",
];

/// Suffixes of private networks that are not in the public suffix list (`home.arpa` is
/// handled separately because it has two labels).
const INTERNAL_SUFFIXES: &[&str] = &[
    "corp",
    "internal",
    "intranet",
    "lan",
    "local",
    "localdomain",
];

const RESERVED_LAST_LABELS: &[&str] = &["example", "invalid", "localhost", "test"];
const RESERVED_DOMAINS: &[&str] = &["example.com", "example.net", "example.org"];

const LABEL: &str = r"[A-Za-z0-9](?:[A-Za-z0-9-]*[A-Za-z0-9])?";

static CHAIN: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(&format!(r"{LABEL}(?:\.{LABEL})+")).expect("regex"));

fn in_list(list: &[&str], s: &str) -> bool {
    list.binary_search(&s).is_ok()
}

fn last_label(s: &str) -> &str {
    s.rsplit('.').next().unwrap_or(s)
}

fn ends_with_domain(name: &str, domain: &str) -> bool {
    name == domain
        || name
            .strip_suffix(domain)
            .is_some_and(|head| head.ends_with('.'))
}

/// `name` is lower case, without a trailing dot.
fn is_reserved(name: &str) -> bool {
    RESERVED_LAST_LABELS.contains(&last_label(name))
        || RESERVED_DOMAINS.iter().any(|d| ends_with_domain(name, d))
}

fn has_internal_suffix(name: &str) -> bool {
    INTERNAL_SUFFIXES.contains(&last_label(name)) || ends_with_domain(name, "home.arpa")
}

/// True if the public suffix list knows the suffix of `name` (lower case).
fn has_public_suffix(name: &str) -> bool {
    psl::suffix(name.as_bytes()).is_some_and(|s| s.is_known())
}

/// The suffix of a lower-case, trailing-dot-free name with at least two labels: `home.arpa`,
/// else the public suffix, which for an unknown last label is that label.
fn suffix_of(name: &str) -> &str {
    if ends_with_domain(name, "home.arpa") {
        return "home.arpa";
    }
    let suffix = psl::suffix_str(name).unwrap_or_else(|| last_label(name));
    if ends_with_domain(name, suffix) {
        suffix
    } else {
        last_label(name)
    }
}

/// True if the suffix of `host` is a known entry of the public suffix list or one of the
/// internal suffixes `local`, `internal`, `lan`, `corp`, `intranet`, `localdomain` and
/// `home.arpa`. A single-label host has no suffix.
pub fn has_known_suffix(host: &str) -> bool {
    let lower = host.strip_suffix('.').unwrap_or(host).to_ascii_lowercase();
    lower.contains('.') && (has_internal_suffix(&lower) || has_public_suffix(&lower))
}

/// Maps a hostname to a fake one. The public suffix is kept, every other label is replaced
/// by pseudo-text of the same shape, seeded by the name from that label to the end.
pub fn pseudonymize(host: &str, ctx: &Context<'_>) -> String {
    if ctx.is_allowed(host) {
        return host.to_string();
    }
    let (name, dot) = match host.strip_suffix('.') {
        Some(n) => (n, "."),
        None => (host, ""),
    };
    // ASCII lower-casing keeps byte offsets identical to `name`.
    let lower = name.to_ascii_lowercase();
    if is_reserved(&lower) {
        return host.to_string();
    }
    let suffix_len = if lower.contains('.') {
        suffix_of(&lower).len()
    } else {
        0
    };
    let head_end = if suffix_len == 0 {
        name.len()
    } else if suffix_len + 1 >= name.len() {
        // The name is only a suffix.
        return host.to_string();
    } else {
        name.len() - suffix_len - 1
    };
    let mut out = String::with_capacity(host.len());
    let mut offset = 0;
    for label in name[..head_end].split('.') {
        let seed = &lower[offset..];
        let shape = match label.get(..4) {
            Some(p) if p.eq_ignore_ascii_case("xn--") => &label[4..],
            _ => label,
        };
        out.push_str(&ctx.pseudo().mimic("hostname", seed.as_bytes(), shape));
        out.push('.');
        offset += label.len() + 1;
    }
    if suffix_len == 0 {
        out.pop();
    } else {
        out.push_str(&name[head_end + 1..]);
    }
    out.push_str(dot);
    out
}

/// Detects and replaces bare hostnames.
pub struct HostnameRule;

fn is_alpha_label(label: &str) -> bool {
    label.len() >= 2 && label.bytes().all(|b| b.is_ascii_alphabetic())
}

/// Whether the (possibly shortened) chain `s` is reported as a hostname.
fn accepted(s: &str, shortened: bool, followed_by_paren: bool) -> bool {
    let lower = s.to_ascii_lowercase();
    let last = last_label(&lower);
    is_alpha_label(last)
        && lower.len() <= 253
        && if shortened {
            has_public_suffix(&lower)
        } else {
            has_known_suffix(&lower)
        }
        && !in_list(AMBIGUOUS_TLDS, last)
        && !in_list(NOT_HOSTNAMES, &lower)
        && !is_reserved(&lower)
        && (shortened || !followed_by_paren)
}

impl Rule for HostnameRule {
    fn kind(&self) -> Kind {
        Kind::Hostname
    }

    fn detect(&self, text: &str) -> Vec<Range<usize>> {
        let mut out = Vec::new();
        for m in CHAIN.find_iter(text) {
            let (start, end) = (m.start(), m.end());
            if text[..start]
                .chars()
                .next_back()
                .is_some_and(char::is_alphanumeric)
            {
                continue;
            }
            let after = text[end..].chars().next();
            if after.is_some_and(char::is_alphanumeric) {
                continue;
            }
            let chain = m.as_str();
            if chain.split('.').any(|l| l.len() > 63) {
                continue;
            }
            let paren = after == Some('(');
            let mut cand = chain;
            let mut shortened = false;
            loop {
                if accepted(cand, shortened, paren) {
                    out.push(start..start + cand.len());
                    break;
                }
                let Some((head, last)) = cand.rsplit_once('.') else {
                    break;
                };
                if !head.contains('.') || !in_list(FILE_EXTENSIONS, &last.to_ascii_lowercase()) {
                    break;
                }
                cand = head;
                shortened = true;
            }
        }
        out
    }

    fn replace(&self, original: &str, ctx: &Context<'_>) -> String {
        pseudonymize(original, ctx)
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
            vec![Box::new(HostnameRule)],
        )
    }

    fn map(host: &str) -> String {
        let e = engine();
        pseudonymize(host, &e.context())
    }

    fn found(text: &str) -> Vec<&str> {
        HostnameRule
            .detect(text)
            .into_iter()
            .map(|r| &text[r])
            .collect()
    }

    #[test]
    fn lists_are_sorted() {
        for list in [
            AMBIGUOUS_TLDS,
            NOT_HOSTNAMES,
            FILE_EXTENSIONS,
            INTERNAL_SUFFIXES,
        ] {
            assert!(list.windows(2).all(|w| w[0] < w[1]));
        }
    }

    #[test]
    fn hierarchy_shares_parent_mapping() {
        let a = map("git.acme.com");
        let c = map("www.acme.com");
        let b = map("acme.com");
        let (la, ra) = a.split_once('.').unwrap();
        let (lc, rc) = c.split_once('.').unwrap();
        assert_eq!(ra, b);
        assert_eq!(rc, b);
        assert_ne!(la, lc);
        assert!(b.ends_with(".com") && b != "acme.com");
    }

    #[test]
    fn shape_is_kept() {
        let out = map("db-prod-01.corp.internal");
        assert!(out.ends_with(".internal"));
        let labels: Vec<&str> = out.split('.').collect();
        assert_eq!(labels.len(), 3);
        let orig = ["db-prod-01", "corp"];
        for (o, n) in orig.iter().zip(&labels) {
            assert_eq!(o.len(), n.len());
            for (a, b) in o.chars().zip(n.chars()) {
                assert_eq!(a.is_ascii_digit(), b.is_ascii_digit(), "{out}");
                assert_eq!(a == '-', b == '-', "{out}");
                assert_eq!(a.is_ascii_alphabetic(), b.is_ascii_alphabetic(), "{out}");
            }
            assert_ne!(o, n);
        }
    }

    #[test]
    fn case_and_trailing_dot() {
        let lower = map("acme.com");
        let (label, _) = lower.split_once('.').unwrap();
        assert_eq!(map("ACME.com"), format!("{}.com", label.to_uppercase()));
        assert_eq!(map("acme.com."), format!("{lower}."));
    }

    #[test]
    fn suffix_is_kept() {
        assert!(map("www.acme.co.uk").ends_with(".co.uk"));
        assert!(map("acme.github.io").ends_with(".github.io"));
        assert!(map("printer.home.arpa").ends_with(".home.arpa"));
        assert_ne!(map("acme.github.io"), "acme.github.io");
        assert_eq!(map("co.uk"), "co.uk");
        assert_eq!(map("github.io"), "github.io");
    }

    #[test]
    fn reserved_names_untouched() {
        for h in [
            "localhost",
            "example.com",
            "www.example.org",
            "a.b.example.net",
            "foo.test",
            "x.invalid",
            "foo.localhost",
            "my.example",
        ] {
            assert_eq!(map(h), h);
        }
        assert_ne!(map("notexample.com"), "notexample.com");
    }

    #[test]
    fn single_label_and_punycode() {
        let m = map("db-prod-01");
        assert_ne!(m, "db-prod-01");
        assert_eq!(m.len(), 10);
        let x = map("xn--bcher-kva.acme.de");
        assert!(!x.contains("xn--"));
        assert!(x.ends_with(".de"));
    }

    #[test]
    fn known_suffixes() {
        for h in [
            "a.com",
            "a.co.uk",
            "a.github.io",
            "a.local",
            "a.internal",
            "a.lan",
            "a.corp",
            "a.intranet",
            "a.localdomain",
            "a.home.arpa",
            "A.COM.",
        ] {
            assert!(has_known_suffix(h), "{h}");
        }
        for h in ["a.foo1", "a.string", "com", "localhost", "a.conf"] {
            assert!(!has_known_suffix(h), "{h}");
        }
    }

    #[test]
    fn must_match() {
        let cases: [(&str, &str); 14] = [
            ("git.acme.com", "git.acme.com"),
            ("acme.com", "acme.com"),
            ("www.acme.co.uk", "www.acme.co.uk"),
            ("db01.corp.internal", "db01.corp.internal"),
            ("printer.local", "printer.local"),
            ("acme.io", "acme.io"),
            ("(git.acme.com)", "git.acme.com"),
            ("see git.acme.com.", "git.acme.com"),
            ("git.acme.com:22", "git.acme.com"),
            ("*.acme.com", "acme.com"),
            ("/etc/letsencrypt/live/acme.com/fullchain.pem", "acme.com"),
            ("sites-enabled/acme.com.conf", "acme.com"),
            ("backup_acme.com.tar.gz", "acme.com"),
            ("_dmarc.acme.com", "dmarc.acme.com"),
        ];
        for (text, want) in cases {
            assert_eq!(found(text), vec![want], "{text}");
        }
    }

    #[test]
    fn dmarc_keeps_underscore() {
        assert_eq!(
            engine().obfuscate("_dmarc.acme.com"),
            format!("_{}", map("dmarc.acme.com"))
        );
    }

    #[test]
    fn must_not_match() {
        for text in [
            "main.rs",
            "README.md",
            "script.py",
            "Main.java",
            "main.tf",
            "archive.zip",
            "Cargo.toml",
            "index.html",
            "jquery.min.js",
            "settings.local.json",
            ".env.local",
            "user.name",
            "item.id",
            "foo.bar",
            "e.g.",
            "v1.2.3",
            "1.2.3.4",
            "java.lang.String",
            "java.io.File",
            "import java.io.*;",
            "using System.Net;",
            "com.acme.Service.run(x)",
            "process.env.NODE_ENV",
            "obj.com(x)",
            "localhost",
            "www.example.org",
            "foo.test",
            "acme.comé",
            "éacme.com",
        ] {
            assert!(found(text).is_empty(), "{text}: {:?}", found(text));
        }
    }

    #[test]
    fn label_and_length_limits() {
        let long = "a".repeat(64);
        assert!(found(&format!("{long}.com")).is_empty());
        let ok = "a".repeat(63);
        assert_eq!(found(&format!("{ok}.com")).len(), 1);
        let too_long = format!("{0}.{0}.{0}.{0}.com", "a".repeat(62));
        assert!(found(&too_long).is_empty());
    }

    #[test]
    fn engine_replaces_consistently() {
        let e = engine();
        assert_eq!(
            e.obfuscate("ssh git.acme.com, git.acme.com"),
            format!("ssh {0}, {0}", map("git.acme.com"))
        );
    }
}
