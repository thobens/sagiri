//! IPv4 and IPv6 address rules.

use std::net::{Ipv4Addr, Ipv6Addr};
use std::ops::Range;
use std::str::FromStr;
use std::sync::LazyLock;

use regex::Regex;

use crate::pseudo::Pseudonymizer;
use crate::rule::{Context, Kind, Rule};

static V4_CANDIDATE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"[0-9]{1,3}\.[0-9]{1,3}\.[0-9]{1,3}\.[0-9]{1,3}").expect("regex"));

static V6_RUN: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"[0-9A-Fa-f:.]+").expect("regex"));

fn char_before(text: &str, at: usize) -> Option<char> {
    text[..at].chars().next_back()
}

fn char_after(text: &str, at: usize) -> Option<char> {
    text[at..].chars().next()
}

/// True if the text after `end` is a `.` followed by an alphanumeric character.
fn dot_then_alnum(text: &str, end: usize) -> bool {
    let mut it = text[end..].chars();
    it.next() == Some('.') && it.next().is_some_and(char::is_alphanumeric)
}

/// Parses dotted decimal, accepting leading zeros (read as decimal).
fn parse_v4_lenient(s: &str) -> Option<Ipv4Addr> {
    let mut octets = [0u8; 4];
    let mut parts = s.split('.');
    for o in &mut octets {
        let part = parts.next()?;
        if part.is_empty() || part.len() > 3 || !part.bytes().all(|b| b.is_ascii_digit()) {
            return None;
        }
        *o = part
            .parse::<u16>()
            .ok()
            .and_then(|v| u8::try_from(v).ok())?;
    }
    if parts.next().is_some() {
        return None;
    }
    Some(Ipv4Addr::from(octets))
}

/// Addresses that are never replaced: 0/8, 127/8, 224/4 and 240/4.
fn v4_never_replaced(addr: Ipv4Addr) -> bool {
    let first = addr.octets()[0];
    first == 0 || first == 127 || first >= 224
}

fn v4_in_block(v: u32, net: u32, bits: u32) -> bool {
    v >> (32 - bits) == net >> (32 - bits)
}

const NET_10: u32 = 0x0a00_0000;
const NET_172_16: u32 = 0xac10_0000;
const NET_192_168: u32 = 0xc0a8_0000;
const NET_169_254: u32 = 0xa9fe_0000;
const NET_100_64: u32 = 0x6440_0000;

fn v4_is_public(v: u32) -> bool {
    !v4_never_replaced(Ipv4Addr::from(v))
        && !v4_in_block(v, NET_10, 8)
        && !v4_in_block(v, NET_172_16, 12)
        && !v4_in_block(v, NET_192_168, 16)
        && !v4_in_block(v, NET_169_254, 16)
        && !v4_in_block(v, NET_100_64, 10)
}

/// Maps an IPv4 address to another one of the same category, injectively.
/// Never-replaced addresses are returned unchanged.
pub fn pseudonymize_v4(addr: Ipv4Addr, p: &Pseudonymizer) -> Ipv4Addr {
    if v4_never_replaced(addr) {
        return addr;
    }
    let v = u32::from(addr);
    let within = |domain: &str, net: u32, bits: u32| -> u32 {
        let low_bits = 32 - bits;
        let low = u64::from(v & ((1u32 << low_bits) - 1));
        let mapped = p.permute(domain, low_bits, low);
        net | mapped as u32
    };
    let out = if v4_in_block(v, NET_10, 8) {
        within("ipv4/10", NET_10, 8)
    } else if v4_in_block(v, NET_172_16, 12) {
        within("ipv4/172.16", NET_172_16, 12)
    } else if v4_in_block(v, NET_192_168, 16) {
        within("ipv4/192.168", NET_192_168, 16)
    } else if v4_in_block(v, NET_169_254, 16) {
        within("ipv4/169.254", NET_169_254, 16)
    } else if v4_in_block(v, NET_100_64, 10) {
        within("ipv4/100.64", NET_100_64, 10)
    } else {
        p.permute_within("ipv4/public", 32, u64::from(v), |x| v4_is_public(x as u32)) as u32
    };
    Ipv4Addr::from(out)
}

/// Addresses that are never replaced: `::`, `::1` and `ff00::/8`.
fn v6_never_replaced(addr: Ipv6Addr) -> bool {
    addr == Ipv6Addr::UNSPECIFIED || addr == Ipv6Addr::LOCALHOST || addr.octets()[0] == 0xff
}

/// True if [`pseudonymize_v6`] returns `addr` unchanged: a never-replaced address, or an
/// IPv4-mapped address whose embedded address is never replaced.
fn v6_kept(addr: Ipv6Addr) -> bool {
    v6_never_replaced(addr) || addr.to_ipv4_mapped().is_some_and(v4_never_replaced)
}

/// Maps an IPv6 address to another one of the same category.
/// Never-replaced addresses are returned unchanged.
pub fn pseudonymize_v6(addr: Ipv6Addr, p: &Pseudonymizer) -> Ipv6Addr {
    if v6_never_replaced(addr) {
        return addr;
    }
    if let Some(v4) = addr.to_ipv4_mapped() {
        return pseudonymize_v4(v4, p).to_ipv6_mapped();
    }
    let orig = addr.octets();
    let mut out = orig;
    if addr.segments()[0] & 0xffc0 == 0xfe80 {
        p.stream("ipv6/link-local", &orig).fill(&mut out[8..]);
    } else if orig[0] & 0xfe == 0xfc {
        p.stream("ipv6/ula", &orig).fill(&mut out[1..]);
    } else if orig[0] & 0xe0 == 0x20 {
        // The stream fills all 16 bytes, then the top 3 bits of the first byte are set
        // back to the original `001` prefix, so the low 125 bits come from the stream.
        // The golden vectors depend on this layout.
        p.stream("ipv6/global", &orig).fill(&mut out);
        out[0] = (orig[0] & 0xe0) | (out[0] & 0x1f);
    } else {
        p.stream("ipv6/other", &orig).fill(&mut out[2..]);
    }
    Ipv6Addr::from(out)
}

/// Detects and replaces IPv4 addresses.
pub struct Ipv4Rule;

impl Rule for Ipv4Rule {
    fn kind(&self) -> Kind {
        Kind::Ipv4
    }

    fn detect(&self, text: &str) -> Vec<Range<usize>> {
        let mut out = Vec::new();
        for m in V4_CANDIDATE.find_iter(text) {
            let (start, end) = (m.start(), m.end());
            if char_before(text, start).is_some_and(|c| c.is_alphanumeric() || c == '_' || c == '.')
            {
                continue;
            }
            if char_after(text, end).is_some_and(|c| c.is_alphanumeric() || c == '_')
                || dot_then_alnum(text, end)
            {
                continue;
            }
            if parse_v4_lenient(m.as_str()).is_some_and(|a| !v4_never_replaced(a)) {
                out.push(start..end);
            }
        }
        out
    }

    fn replace(&self, original: &str, ctx: &Context<'_>) -> String {
        match parse_v4_lenient(original) {
            Some(a) => pseudonymize_v4(a, ctx.pseudo()).to_string(),
            None => original.to_string(),
        }
    }
}

/// Detects and replaces IPv6 addresses.
pub struct Ipv6Rule;

impl Rule for Ipv6Rule {
    fn kind(&self) -> Kind {
        Kind::Ipv6
    }

    fn detect(&self, text: &str) -> Vec<Range<usize>> {
        let mut out = Vec::new();
        for m in V6_RUN.find_iter(text) {
            let start = m.start();
            if m.as_str().bytes().filter(|&b| b == b':').count() < 2 {
                continue;
            }
            if char_before(text, start).is_some_and(char::is_alphanumeric) {
                continue;
            }
            // Trailing dots are sentence punctuation, not part of the address.
            let trimmed = m.as_str().trim_end_matches('.');
            let end = start + trimmed.len();
            if char_after(text, end).is_some_and(char::is_alphanumeric) || dot_then_alnum(text, end)
            {
                continue;
            }
            if Ipv6Addr::from_str(trimmed).is_ok_and(|a| !v6_kept(a)) {
                out.push(start..end);
            }
        }
        out
    }

    fn replace(&self, original: &str, ctx: &Context<'_>) -> String {
        match Ipv6Addr::from_str(original) {
            Ok(a) => pseudonymize_v6(a, ctx.pseudo()).to_string(),
            Err(_) => original.to_string(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::{Engine, render};
    use crate::key::Key;

    fn test_pseudo() -> Pseudonymizer {
        let mut b = [0u8; 32];
        for (i, x) in b.iter_mut().enumerate() {
            *x = i as u8;
        }
        Pseudonymizer::new(&Key::from_bytes(b))
    }

    fn engine() -> Engine {
        Engine::new(test_pseudo(), vec![Box::new(Ipv4Rule), Box::new(Ipv6Rule)])
    }

    fn found(text: &str) -> Vec<String> {
        engine()
            .analyze(text)
            .into_iter()
            .map(|f| f.original)
            .collect()
    }

    fn detect_all(text: &str) -> Vec<String> {
        let mut v: Vec<Range<usize>> = Ipv4Rule.detect(text);
        v.extend(Ipv6Rule.detect(text));
        v.sort_by_key(|r| (r.start, std::cmp::Reverse(r.end)));
        v.into_iter().map(|r| text[r].to_string()).collect()
    }

    #[test]
    fn detection_table() {
        let cases = [
            ("10.0.0.1", "10.0.0.1"),
            ("(10.0.0.1)", "10.0.0.1"),
            ("10.0.0.1:8080", "10.0.0.1"),
            ("10.0.0.0/8", "10.0.0.0"),
            ("ip=10.0.0.1,", "10.0.0.1"),
            ("010.001.000.001", "010.001.000.001"),
            ("[2001:db8::1]:443", "2001:db8::1"),
            ("fe80::1%eth0", "fe80::1"),
            ("2001:db8::/32", "2001:db8::"),
            ("::ffff:192.168.1.10", "::ffff:192.168.1.10"),
            ("at 10.0.0.1.", "10.0.0.1"),
            ("at 2001:db8::1.", "2001:db8::1"),
        ];
        for (text, want) in cases {
            assert_eq!(found(text), vec![want.to_string()], "{text}");
        }
    }

    #[test]
    fn rejection_table() {
        for text in [
            "256.1.1.1",
            "1.2.3.4.5",
            "v1.2.3.4",
            "libfoo-1.2.3.4.tar.gz",
            "12:34:56",
            "aa:bb:cc:dd:ee:ff",
            "1.2.3",
            "x10.0.0.1",
            "_10.0.0.1",
            "10.0.0.1_",
            "x2001:db8::1",
            "a.2001:db8::1",
            "2001:db8::1:",
            "2001:db8::1.com",
            "2001:db8::1.net",
            "2001:db8::1.org",
            "host.10.0.0.1",
            "x.10.0.0.1",
            "2001:db8::1g",
        ] {
            assert!(detect_all(text).is_empty(), "{text}");
        }
    }

    #[test]
    fn never_replaced_not_detected_or_changed() {
        for text in [
            "0.0.0.0",
            "127.0.0.1",
            "255.255.255.0",
            "255.255.255.255",
            "224.0.0.251",
            "::",
            "::1",
            "ff02::1",
            "::ffff:127.0.0.1",
            "::ffff:0.0.0.0",
            "::ffff:224.0.0.251",
        ] {
            assert!(detect_all(text).is_empty(), "{text}");
            assert_eq!(engine().obfuscate(text), text);
        }
    }

    fn category(a: Ipv4Addr) -> u8 {
        let v = u32::from(a);
        if v4_in_block(v, NET_10, 8) {
            1
        } else if v4_in_block(v, NET_172_16, 12) {
            2
        } else if v4_in_block(v, NET_192_168, 16) {
            3
        } else if v4_in_block(v, NET_169_254, 16) {
            4
        } else if v4_in_block(v, NET_100_64, 10) {
            5
        } else if v4_is_public(v) {
            6
        } else {
            0
        }
    }

    #[test]
    fn category_table() {
        let p = test_pseudo();
        for (s, cat) in [
            ("10.1.2.3", 1),
            ("172.16.5.6", 2),
            ("172.31.255.255", 2),
            ("192.168.1.1", 3),
            ("169.254.9.9", 4),
            ("100.64.0.1", 5),
            ("100.127.255.254", 5),
            ("172.55.0.1", 6),
            ("198.51.100.8", 6),
            ("203.0.113.7", 6),
        ] {
            let a: Ipv4Addr = s.parse().unwrap();
            assert_eq!(category(a), cat, "{s}");
            let m = pseudonymize_v4(a, &p);
            assert_eq!(category(m), cat, "{s} -> {m}");
        }
    }

    #[test]
    fn ipv4_192_168_is_bijective() {
        let p = test_pseudo();
        let mut seen = vec![false; 65536];
        for low in 0..65536u32 {
            let m = u32::from(pseudonymize_v4(Ipv4Addr::from(NET_192_168 | low), &p));
            assert_eq!(m >> 16, NET_192_168 >> 16);
            let idx = (m & 0xffff) as usize;
            assert!(!seen[idx], "collision");
            seen[idx] = true;
        }
    }

    #[test]
    fn ten_and_public_samples_do_not_collide() {
        use std::collections::HashSet;
        let p = test_pseudo();

        // Stride 167 is odd, so inputs are distinct modulo 2^24.
        let mut seen = HashSet::new();
        for i in 0..100_000u32 {
            let a = NET_10 | (i.wrapping_mul(167) & 0x00ff_ffff);
            let m = pseudonymize_v4(Ipv4Addr::from(a), &p);
            assert_eq!(category(m), 1);
            seen.insert(u32::from(m));
        }
        assert_eq!(seen.len(), 100_000);

        let mut seen = HashSet::new();
        let mut inputs = HashSet::new();
        let mut x = 0x0100_0001u32;
        while inputs.len() < 100_000 {
            x = x.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            if !v4_is_public(x) || !inputs.insert(x) {
                continue;
            }
            let m = pseudonymize_v4(Ipv4Addr::from(x), &p);
            assert!(v4_is_public(u32::from(m)));
            seen.insert(u32::from(m));
        }
        assert_eq!(seen.len(), 100_000);
    }

    #[test]
    fn equivalent_spellings_map_equal() {
        let e = engine();
        assert_eq!(e.obfuscate("010.001.000.001"), e.obfuscate("10.1.0.1"));
        assert_ne!(e.obfuscate("10.1.0.1"), "10.1.0.1");
        assert_eq!(
            e.obfuscate("2001:DB8::1"),
            e.obfuscate("2001:db8:0:0:0:0:0:1")
        );
        assert_ne!(e.obfuscate("2001:db8::1"), "2001:db8::1");
    }

    #[test]
    fn mapped_uses_v4_mapping() {
        let p = test_pseudo();
        let v4 = pseudonymize_v4("192.168.1.10".parse().unwrap(), &p);
        let m = pseudonymize_v6("::ffff:192.168.1.10".parse().unwrap(), &p);
        assert_eq!(m.to_string(), format!("::ffff:{v4}"));
        let f = engine().analyze("::ffff:192.168.1.10");
        assert_eq!(f.len(), 1);
        assert_eq!(f[0].kind, Kind::Ipv6);
    }

    #[test]
    fn v6_categories_preserved() {
        let p = test_pseudo();
        let a: Ipv6Addr = "fe80::1234:5678".parse().unwrap();
        let m = pseudonymize_v6(a, &p);
        assert_eq!(m.octets()[..8], a.octets()[..8]);
        assert_ne!(m, a);
        let a: Ipv6Addr = "fd12:3456::1".parse().unwrap();
        let m = pseudonymize_v6(a, &p);
        assert_eq!(m.octets()[0], 0xfd);
        assert_ne!(m, a);
        let a: Ipv6Addr = "2001:db8::1".parse().unwrap();
        let m = pseudonymize_v6(a, &p);
        assert_eq!(m.octets()[0] & 0xe0, 0x20);
        assert_ne!(m, a);
        let a: Ipv6Addr = "64:ff9b::1".parse().unwrap();
        let m = pseudonymize_v6(a, &p);
        assert_eq!(m.octets()[..2], a.octets()[..2]);
        assert_ne!(m, a);
    }

    #[test]
    fn surrounding_text_is_kept() {
        let text = "a (10.0.0.1:80) b\n[2001:db8::1]:443 /8 10.0.0.0/8 fe80::1%eth0\n";
        let e = engine();
        let findings = e.analyze(text);
        assert_eq!(findings.len(), 4);
        let r = render(text, &findings, |_| true);
        assert_eq!(r.text, e.obfuscate(text));
        assert_eq!(r.spans.len(), findings.len());
        // Every gap between findings has the same bytes in input and output.
        let (mut pos_in, mut pos_out) = (0, 0);
        for s in &r.spans {
            let f = &findings[s.finding];
            assert!(s.replaced);
            assert_eq!(r.text[s.range.clone()], f.replacement);
            assert_eq!(r.text[pos_out..s.range.start], text[pos_in..f.span.start]);
            pos_in = f.span.end;
            pos_out = s.range.end;
        }
        assert_eq!(r.text[pos_out..], text[pos_in..]);
    }
}
