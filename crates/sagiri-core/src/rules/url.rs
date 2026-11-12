//! URL rule: replaces the sensitive parts of a URL and keeps everything else byte-identical.

use std::ops::Range;
use std::sync::LazyLock;

use regex::Regex;

use crate::pseudo::Pseudonymizer;
use crate::rule::{Context, Kind, Rule};
use crate::rules::hostname;

static URL_CANDIDATE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r#"[A-Za-z][A-Za-z0-9+.\-]*://[^\s<>"`]+"#).expect("regex"));

/// Detects and replaces URLs with a `scheme://` prefix.
pub struct UrlRule;

/// Length of `url` after trimming trailing punctuation and unmatched closing brackets.
fn trimmed_len(url: &str) -> usize {
    let mut s = url;
    while let Some(last) = s.chars().next_back() {
        let opener = match last {
            ')' => '(',
            ']' => '[',
            '}' => '{',
            '.' | ',' | ';' | ':' | '!' | '?' | '\'' => {
                s = &s[..s.len() - 1];
                continue;
            }
            _ => break,
        };
        let opens = s.chars().filter(|&c| c == opener).count();
        let closes = s.chars().filter(|&c| c == last).count();
        if closes > opens {
            s = &s[..s.len() - 1];
        } else {
            break;
        }
    }
    s.len()
}

impl Rule for UrlRule {
    fn kind(&self) -> Kind {
        Kind::Url
    }

    fn detect(&self, text: &str) -> Vec<Range<usize>> {
        let mut out = Vec::new();
        for m in URL_CANDIDATE.find_iter(text) {
            let start = m.start();
            if text[..start]
                .chars()
                .next_back()
                .is_some_and(char::is_alphanumeric)
            {
                continue;
            }
            let url = m.as_str();
            let end = start + trimmed_len(url);
            let Some(sep) = url.find("://") else {
                continue;
            };
            if end > start + sep + 3 {
                out.push(start..end);
            }
        }
        out
    }

    fn replace(&self, original: &str, ctx: &Context<'_>) -> String {
        replace_url(original, ctx)
    }
}

/// Applies `mimic` to the pieces of `value` between percent escapes; escapes are copied.
/// Piece number `n` (counting from 0, one per preceding escape) is seeded with the whole
/// value followed by `n` as 4 big-endian bytes.
fn mimic_keeping_escapes(p: &Pseudonymizer, domain: &str, value: &str) -> String {
    let b = value.as_bytes();
    let mut out = String::with_capacity(value.len());
    let (mut start, mut i) = (0, 0);
    let mut index: u32 = 0;
    let mut flush = |out: &mut String, from: usize, to: usize| {
        if from < to {
            let mut seed = b.to_vec();
            seed.extend_from_slice(&index.to_be_bytes());
            out.push_str(&p.mimic(domain, &seed, &value[from..to]));
        }
        index += 1;
    };
    while i < b.len() {
        if b[i] == b'%'
            && i + 2 < b.len()
            && b[i + 1].is_ascii_hexdigit()
            && b[i + 2].is_ascii_hexdigit()
        {
            flush(&mut out, start, i);
            out.push_str(&value[i..i + 3]);
            i += 3;
            start = i;
        } else {
            i += 1;
        }
    }
    flush(&mut out, start, b.len());
    out
}

fn looks_like_ipv4(host: &str) -> bool {
    let mut n = 0;
    for part in host.split('.') {
        n += 1;
        if part.is_empty() || part.len() > 3 || !part.bytes().all(|b| b.is_ascii_digit()) {
            return false;
        }
    }
    n == 4
}

fn replace_userinfo(userinfo: &str, p: &Pseudonymizer) -> String {
    let (user, pass) = match userinfo.split_once(':') {
        Some((u, pw)) => (u, Some(pw)),
        None => (userinfo, None),
    };
    let mut out = mimic_keeping_escapes(p, "url-userinfo", user);
    if let Some(pw) = pass {
        out.push(':');
        out.push_str(&mimic_keeping_escapes(p, "url-userinfo", pw));
    }
    out
}

fn replace_host_port(hostport: &str, ctx: &Context<'_>) -> String {
    if let Some(inner_and_rest) = hostport.strip_prefix('[') {
        return match inner_and_rest.split_once(']') {
            Some((inner, rest)) => format!("[{}]{rest}", ctx.nested(inner)),
            None => hostport.to_string(),
        };
    }
    let (host, port) = match hostport.rfind(':') {
        Some(i) if hostport[i + 1..].bytes().all(|b| b.is_ascii_digit()) => hostport.split_at(i),
        _ => (hostport, ""),
    };
    let new_host = if host.is_empty() {
        String::new()
    } else if host.contains(':') || looks_like_ipv4(host) {
        ctx.nested(host)
    } else if host.bytes().all(|b| b.is_ascii_digit() || b == b'.') {
        // Digits and dots that are not an IPv4 address (`1.2.3`): left as they are.
        host.to_string()
    } else {
        hostname::pseudonymize(host, ctx)
    };
    format!("{new_host}{port}")
}

fn replace_query(query: &str, ctx: &Context<'_>) -> String {
    let pairs: Vec<String> = query
        .split('&')
        .map(|pair| match pair.split_once('=') {
            Some((key, value)) if !value.is_empty() => {
                let mut new = ctx.nested(value);
                if new == value {
                    new = mimic_keeping_escapes(ctx.pseudo(), "url-query", value);
                }
                format!("{key}={new}")
            }
            _ => pair.to_string(),
        })
        .collect();
    pairs.join("&")
}

fn replace_url(url: &str, ctx: &Context<'_>) -> String {
    let Some(sep) = url.find("://") else {
        return url.to_string();
    };
    let (scheme, rest) = url.split_at(sep + 3);
    let auth_end = rest.find(['/', '?', '#']).unwrap_or(rest.len());
    let (authority, tail) = rest.split_at(auth_end);
    let (before_fragment, fragment) = match tail.split_once('#') {
        Some((a, f)) => (a, Some(f)),
        None => (tail, None),
    };
    let (path, query) = match before_fragment.split_once('?') {
        Some((p, q)) => (p, Some(q)),
        None => (before_fragment, None),
    };

    let mut out = String::with_capacity(url.len());
    out.push_str(scheme);
    let hostport = match authority.rsplit_once('@') {
        Some((userinfo, hostport)) => {
            out.push_str(&replace_userinfo(userinfo, ctx.pseudo()));
            out.push('@');
            hostport
        }
        None => authority,
    };
    out.push_str(&replace_host_port(hostport, ctx));
    if !path.is_empty() {
        out.push_str(&ctx.nested(path));
    }
    if let Some(q) = query {
        out.push('?');
        out.push_str(&replace_query(q, ctx));
    }
    if let Some(f) = fragment {
        out.push('#');
        out.push_str(&ctx.nested(f));
    }
    if out == url { url.to_string() } else { out }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::Engine;
    use crate::key::Key;
    use crate::rules::default_rules;

    fn engine() -> Engine {
        let mut b = [0u8; 32];
        for (i, x) in b.iter_mut().enumerate() {
            *x = i as u8;
        }
        Engine::new(Pseudonymizer::new(&Key::from_bytes(b)), default_rules())
    }

    fn found(text: &str) -> Vec<&str> {
        UrlRule.detect(text).into_iter().map(|r| &text[r]).collect()
    }

    #[test]
    fn trailing_punctuation() {
        let cases = [
            ("See https://git.acme.com/x.", "https://git.acme.com/x"),
            ("(https://git.acme.com/x)", "https://git.acme.com/x"),
            ("<https://git.acme.com/x>", "https://git.acme.com/x"),
            ("[t](https://git.acme.com/x)", "https://git.acme.com/x"),
            ("\"https://git.acme.com/x\"", "https://git.acme.com/x"),
            ("https://git.acme.com/x?!,;:'", "https://git.acme.com/x"),
            (
                "https://acme.com/wiki/Foo_(bar)",
                "https://acme.com/wiki/Foo_(bar)",
            ),
            ("https://acme.com/a[0]", "https://acme.com/a[0]"),
        ];
        for (text, want) in cases {
            assert_eq!(found(text), vec![want], "{text}");
        }
    }

    #[test]
    fn not_detected() {
        for text in [
            "éhttps://acme.com/",
            "1http://a.b",
            "mailto:a@b.com",
            "http://",
            "http://.",
        ] {
            assert!(found(text).is_empty(), "{text}");
        }
    }

    #[test]
    fn host_equals_bare_host() {
        let e = engine();
        let host = e.obfuscate("git.acme.com");
        assert_ne!(host, "git.acme.com");
        assert_eq!(
            e.obfuscate("https://git.acme.com/team/repo"),
            format!("https://{host}/team/repo")
        );
    }

    #[test]
    fn ip_hosts() {
        let e = engine();
        let v4 = e.obfuscate("10.0.0.5");
        let v6 = e.obfuscate("2001:db8::1");
        assert_ne!(v4, "10.0.0.5");
        assert_eq!(
            e.obfuscate("http://10.0.0.5:8080/health"),
            format!("http://{v4}:8080/health")
        );
        assert_eq!(
            e.obfuscate("http://[2001:db8::1]:8080/x"),
            format!("http://[{v6}]:8080/x")
        );
        assert_eq!(
            e.obfuscate("http://[2001:db8::1]/x"),
            format!("http://[{v6}]/x")
        );
        assert_eq!(
            e.obfuscate("http://127.0.0.1:80/x"),
            "http://127.0.0.1:80/x"
        );
    }

    #[test]
    fn postgres_url() {
        let e = engine();
        let text = "postgres://admin:s3cret@db01.corp.internal:5432/app?sslmode=require";
        let out = e.obfuscate(text);
        let rest = out.strip_prefix("postgres://").unwrap();
        let (userinfo, rest) = rest.split_once('@').unwrap();
        let (user, pass) = userinfo.split_once(':').unwrap();
        assert!(user.len() == 5 && user != "admin");
        assert!(pass.len() == 6 && pass != "s3cret");
        let (hostport, rest) = rest.split_once('/').unwrap();
        let (host, port) = hostport.split_once(':').unwrap();
        assert_ne!(host, "db01.corp.internal");
        assert!(host.ends_with(".internal"));
        assert_eq!(port, "5432");
        let (path, query) = rest.split_once('?').unwrap();
        assert_eq!(path, "app");
        let value = query.strip_prefix("sslmode=").unwrap();
        assert!(value.len() == 7 && value != "require");
    }

    #[test]
    fn single_label_host() {
        let out = engine().obfuscate("http://db-prod-01:5432/");
        let host = out
            .strip_prefix("http://")
            .and_then(|s| s.strip_suffix(":5432/"))
            .unwrap();
        assert_eq!(host.len(), 10);
        assert_ne!(host, "db-prod-01");
        assert_eq!(host.matches('-').count(), 2);
    }

    #[test]
    fn nested_values() {
        let e = engine();
        let (a, b) = (e.obfuscate("10.0.0.5"), e.obfuscate("10.0.0.6"));
        let host = e.obfuscate("acme.com");
        assert_eq!(
            e.obfuscate("http://acme.com/nodes/10.0.0.5/status?peer=10.0.0.6"),
            format!("http://{host}/nodes/{a}/status?peer={b}")
        );
    }

    #[test]
    fn reserved_hosts() {
        let e = engine();
        assert!(e.analyze("http://localhost:3000/api").is_empty());
        assert!(e.analyze("https://example.com/").is_empty());
        let out = e.obfuscate("http://localhost:3000/api?token=abc123");
        let token = out
            .strip_prefix("http://localhost:3000/api?token=")
            .unwrap();
        assert_eq!(token.len(), 6);
        assert_ne!(token, "abc123");
    }

    #[test]
    fn byte_fidelity() {
        let e = engine();
        let text = "HTTPS://Git.Acme.COM:443/A%20b?x=1%2F2#Frag";
        let out = e.obfuscate(text);
        assert!(out.starts_with("HTTPS://"));
        // The query value `1%2F2` has digits around an escape: the digits change.
        let rest = out.split_once(":443/A%20b?x=").unwrap().1;
        let (v, frag) = rest.split_once('#').unwrap();
        assert_eq!(frag, "Frag");
        assert!(v.len() == 5 && &v[1..4] == "%2F" && v != "1%2F2");
        let host = out
            .strip_prefix("HTTPS://")
            .unwrap()
            .split(':')
            .next()
            .unwrap();
        let bare = e.obfuscate("git.acme.com");
        let (bare_label, bare_rest) = bare.split_once('.').unwrap();
        let (label, rest) = host.split_once('.').unwrap();
        assert_eq!(
            label,
            format!("{}{}", &bare_label[..1].to_uppercase(), &bare_label[1..])
        );
        assert_eq!(rest.to_lowercase(), bare_rest);
        assert!(host.ends_with(".COM"));
    }

    #[test]
    fn escapes_in_query_value_are_kept() {
        let out = engine().obfuscate("http://acme.com/?q=ab%2Fcd%41ef");
        let q = out.split_once("?q=").unwrap().1;
        assert_eq!(q.len(), 12);
        assert_eq!(&q[2..5], "%2F");
        assert_eq!(&q[7..10], "%41");
        assert_ne!(&q[..2], "ab");
        assert_ne!(&q[5..7], "cd");
        assert_ne!(&q[10..], "ef");
        // Pieces have their own seeds: three different-looking pieces.
        assert!(q[..2] != q[5..7] && q[..2] != q[10..] && q[5..7] != q[10..]);
        let same = engine().obfuscate("http://acme.com/?q=ab%2Fab%41ab");
        let q = same.split_once("?q=").unwrap().1;
        assert!(
            q[..2] != q[5..7] && q[5..7] != q[10..] && q[..2] != q[10..],
            "{q}"
        );
    }

    #[test]
    fn escape_only_values_are_left_unchanged() {
        // Known limitation, pinned so that a later ticket changes it on purpose.
        let e = engine();
        assert!(e.analyze("http://localhost/?q=%41%42").is_empty());
        assert!(
            e.analyze("http://localhost/?city=%E6%9D%B1%E4%BA%AC")
                .is_empty()
        );
        assert!(
            e.analyze("ftp://bob:%41%42%43@localhost/")
                .iter()
                .all(|f| f.original.contains("bob"))
        );
        let out = e.obfuscate("ftp://bob:%41%42%43@localhost/");
        assert!(out.ends_with(":%41%42%43@localhost/"));
    }

    #[test]
    fn escape_boundary_does_not_cover_ip_rules() {
        // Known limitation: the IPv4 rule treats `C` in `%2C` as a word character.
        let e = engine();
        let a = e.obfuscate("10.0.0.5");
        let out = e.obfuscate("http://localhost/?p=10.0.0.5%2C10.0.0.6");
        assert_eq!(out, format!("http://localhost/?p={a}%2C10.0.0.6"));
    }

    #[test]
    fn userinfo_escape_is_kept() {
        let out = engine().obfuscate("ftp://bob:p%40ss@localhost/");
        let pass = out.split(':').nth(2).unwrap().split('@').next().unwrap();
        assert_eq!(pass.len(), 6);
        assert_eq!(&pass[1..4], "%40");
        assert_ne!(pass, "p%40ss");
    }

    #[test]
    fn encoded_url_in_query_value_and_path() {
        let e = engine();
        let acme = e.obfuscate("acme.com");
        let git = e.obfuscate("git.acme.com");
        assert_eq!(
            e.obfuscate("https://acme.com/?redirect=https%3A%2F%2Fgit.acme.com%2F"),
            format!("https://{acme}/?redirect=https%3A%2F%2F{git}%2F")
        );
        assert_eq!(
            e.obfuscate("https://acme.com/go/https%3A%2F%2Fgit.acme.com%2Fx"),
            format!("https://{acme}/go/https%3A%2F%2F{git}%2Fx")
        );
    }

    #[test]
    fn digits_and_dots_host_that_is_not_ipv4_is_kept() {
        let e = engine();
        assert!(e.analyze("http://1.2.3/x").is_empty());
        assert!(e.analyze("http://1.2.3.4.5/x").is_empty());
    }

    #[test]
    fn ip_prefixed_dns_name_maps_label_wise() {
        // `10.0.0.5.nip.io` is a hostname; the digit labels are mimicked, not IP-mapped.
        assert_eq!(engine().obfuscate("10.0.0.5.nip.io"), "97.9.9.8.dun.io");
    }

    #[test]
    fn same_query_value_same_replacement() {
        let e = engine();
        let a = e.obfuscate("http://acme.com/a?token=abc123");
        let b = e.obfuscate("https://other-host.net/b?x=1&token=abc123");
        let ta = a.split_once("token=").unwrap().1;
        let tb = b.split_once("token=").unwrap().1;
        assert_eq!(ta, tb);
        assert_ne!(ta, "abc123");
    }

    #[test]
    fn keys_empty_values_and_valueless_pairs_kept() {
        let e = engine();
        let out = e.obfuscate("http://localhost/?a=&flag&b=xyz");
        assert!(out.starts_with("http://localhost/?a=&flag&b="));
        assert_ne!(out, "http://localhost/?a=&flag&b=xyz");
    }

    #[test]
    fn nothing_to_replace_means_no_finding() {
        let e = engine();
        for t in [
            "http://localhost/",
            "file:///etc/hosts",
            "https://www.example.org/a?b=",
        ] {
            assert!(e.analyze(t).is_empty(), "{t}");
        }
    }

    #[test]
    fn userinfo_without_password() {
        let out = engine().obfuscate("ftp://bob@localhost/");
        let user = out
            .strip_prefix("ftp://")
            .unwrap()
            .split('@')
            .next()
            .unwrap();
        assert_eq!(user.len(), 3);
        assert_ne!(user, "bob");
    }

    #[test]
    fn line_count_is_preserved() {
        let text = "a https://git.acme.com/x\nb https://acme.com/?q=1\n";
        assert_eq!(engine().obfuscate(text).lines().count(), 2);
    }
}
