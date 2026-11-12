//! IBAN rule: registry country and length, mod-97 checksum.

use std::ops::Range;

use crate::rule::{Context, Kind, Rule};

/// Country code to total IBAN length, embedded from the SWIFT IBAN registry.
#[rustfmt::skip]
const LENGTHS: &[(&str, usize)] = &[
    ("AD", 24), ("AE", 23), ("AL", 28), ("AT", 20), ("AZ", 28), ("BA", 20), ("BE", 16),
    ("BG", 22), ("BH", 22), ("BI", 27), ("BR", 29), ("BY", 28), ("CH", 21), ("CR", 22),
    ("CY", 28), ("CZ", 24), ("DE", 22), ("DJ", 27), ("DK", 18), ("DO", 28), ("EE", 20),
    ("EG", 29), ("ES", 24), ("FI", 18), ("FK", 18), ("FO", 18), ("FR", 27), ("GB", 22),
    ("GE", 22), ("GI", 23), ("GL", 18), ("GR", 27), ("GT", 28), ("HR", 21), ("HU", 28),
    ("IE", 22), ("IL", 23), ("IQ", 23), ("IS", 26), ("IT", 27), ("JO", 30), ("KW", 30),
    ("KZ", 20), ("LB", 28), ("LC", 32), ("LI", 21), ("LT", 20), ("LU", 20), ("LV", 21),
    ("LY", 25), ("MC", 27), ("MD", 24), ("ME", 22), ("MK", 19), ("MN", 20), ("MR", 27),
    ("MT", 31), ("MU", 30), ("NI", 28), ("NL", 18), ("NO", 15), ("OM", 23), ("PK", 24),
    ("PL", 28), ("PS", 29), ("PT", 25), ("QA", 29), ("RO", 24), ("RS", 22), ("RU", 33),
    ("SA", 24), ("SC", 31), ("SD", 18), ("SE", 24), ("SI", 19), ("SK", 24), ("SM", 27),
    ("SO", 23), ("ST", 25), ("SV", 28), ("TL", 23), ("TN", 24), ("TR", 26), ("UA", 29),
    ("HN", 28), ("VA", 22), ("VG", 24), ("XK", 20), ("YE", 30),
];

const MAX_LEN: usize = 33;

fn expected_len(country: &str) -> Option<usize> {
    LENGTHS.iter().find(|(c, _)| *c == country).map(|&(_, l)| l)
}

/// Mod 97 of `s` with the first four characters moved to the end. `s` is upper case
/// ASCII alphanumeric.
fn mod97(s: &[u8]) -> u32 {
    let mut r = 0u32;
    for &b in s[4..].iter().chain(&s[..4]) {
        if b.is_ascii_digit() {
            r = (r * 10 + u32::from(b - b'0')) % 97;
        } else {
            r = (r * 100 + u32::from(b - b'A') + 10) % 97;
        }
    }
    r
}

/// Validates an upper-case, space-free IBAN.
fn valid_canonical(s: &[u8]) -> bool {
    if s.len() < 5 || s.len() > MAX_LEN {
        return false;
    }
    let Ok(cc) = std::str::from_utf8(&s[..2]) else {
        return false;
    };
    expected_len(cc) == Some(s.len())
        && s[..2].iter().all(u8::is_ascii_uppercase)
        && s[2..4].iter().all(u8::is_ascii_digit)
        && s[4..]
            .iter()
            .all(|b| b.is_ascii_uppercase() || b.is_ascii_digit())
        && mod97(s) == 1
}

/// Letters all upper case or all lower case (a string without letters counts).
fn consistent_case(s: &str) -> bool {
    !(s.bytes().any(|b| b.is_ascii_uppercase()) && s.bytes().any(|b| b.is_ascii_lowercase()))
}

/// End offsets of the whole-token candidates starting at `start`, longest first.
fn candidates(text: &str, start: usize) -> Vec<usize> {
    let b = text.as_bytes();
    let run_end = |from: usize| {
        let mut p = from;
        while p < b.len() && b[p].is_ascii_alphanumeric() {
            p += 1;
        }
        p
    };
    let first = run_end(start);
    if first - start != 4 {
        return vec![first];
    }
    let mut ends = vec![first];
    let mut pos = first;
    while pos + 1 < b.len() && b[pos] == b' ' && b[pos + 1].is_ascii_alphanumeric() {
        let e = run_end(pos + 1);
        let len = e - (pos + 1);
        if len > 4 || e - start > MAX_LEN + 8 {
            break;
        }
        ends.push(e);
        if len < 4 {
            break;
        }
        pos = e;
    }
    ends.reverse();
    ends
}

fn canonical(s: &str) -> Vec<u8> {
    s.bytes()
        .filter(|&c| c != b' ')
        .map(|c| c.to_ascii_uppercase())
        .collect()
}

/// Detects and replaces IBANs.
pub struct IbanRule;

impl Rule for IbanRule {
    fn kind(&self) -> Kind {
        Kind::Iban
    }

    fn detect(&self, text: &str) -> Vec<Range<usize>> {
        let b = text.as_bytes();
        let mut out = Vec::new();
        let mut i = 0;
        while i < b.len() {
            let starts = b[i].is_ascii_alphabetic()
                && i + 4 <= b.len()
                && b[i + 1].is_ascii_alphabetic()
                && b[i + 2].is_ascii_digit()
                && b[i + 3].is_ascii_digit()
                && text[..i]
                    .chars()
                    .next_back()
                    .is_none_or(|c| !c.is_alphanumeric());
            if !starts {
                i += 1;
                continue;
            }
            let mut found = None;
            for end in candidates(text, i) {
                if text[end..]
                    .chars()
                    .next()
                    .is_some_and(char::is_alphanumeric)
                {
                    continue;
                }
                let m = &text[i..end];
                if consistent_case(m) && valid_canonical(&canonical(m)) {
                    found = Some(end);
                    break;
                }
            }
            match found {
                Some(end) => {
                    out.push(i..end);
                    i = end;
                }
                None => {
                    while i < b.len() && b[i].is_ascii_alphanumeric() {
                        i += 1;
                    }
                }
            }
        }
        out
    }

    fn replace(&self, original: &str, ctx: &Context<'_>) -> String {
        let canon = canonical(original);
        if !valid_canonical(&canon) {
            return original.to_string();
        }
        let lower = original.bytes().any(|c| c.is_ascii_lowercase());
        let mut out = canon.clone();
        let mut stream = ctx.pseudo().stream("iban", &canon);
        for c in &mut out[4..] {
            *c = if c.is_ascii_digit() {
                b'0' + stream.below(10) as u8
            } else {
                b'A' + stream.below(26) as u8
            };
        }
        out[2] = b'0';
        out[3] = b'0';
        let check = 98 - mod97(&out);
        out[2] = b'0' + (check / 10) as u8;
        out[3] = b'0' + (check % 10) as u8;
        if lower {
            out.make_ascii_lowercase();
        }
        let mut it = out.into_iter();
        original
            .chars()
            .map(|c| {
                if c == ' ' {
                    c
                } else {
                    it.next().map_or(c, char::from)
                }
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::Engine;
    use crate::key::Key;
    use crate::pseudo::Pseudonymizer;
    use proptest::prelude::*;

    fn engine() -> Engine {
        let mut b = [0u8; 32];
        for (i, x) in b.iter_mut().enumerate() {
            *x = i as u8;
        }
        Engine::new(
            Pseudonymizer::new(&Key::from_bytes(b)),
            vec![Box::new(IbanRule)],
        )
    }

    #[test]
    fn detects_and_replaces_valid_ibans() {
        let e = engine();
        for s in [
            "DE89 3704 0044 0532 0130 00",
            "DE89370400440532013000",
            "GB82 WEST 1234 5698 7654 32",
            "CH93 0076 2011 6238 5295 7",
            "FR14 2004 1010 0505 0001 3M02 606",
            "NL91 ABNA 0417 1643 00",
            "de89370400440532013000",
        ] {
            let text = format!("iban {s} end");
            let f = e.analyze(&text);
            assert_eq!(f.len(), 1, "{s}");
            assert_eq!(f[0].original, s);
            let r = &f[0].replacement;
            assert_eq!(r.len(), s.len(), "{s}");
            assert!(valid_canonical(&canonical(r)), "{s} -> {r}");
            assert_eq!(r[..2], s[..2]);
            for (a, b) in s.chars().zip(r.chars()) {
                assert_eq!(a == ' ', b == ' ');
                assert_eq!(a.is_ascii_digit(), b.is_ascii_digit(), "{s} -> {r}");
                assert_eq!(a.is_ascii_lowercase(), b.is_ascii_lowercase());
            }
        }
    }

    #[test]
    fn spaced_and_contiguous_agree() {
        let e = engine();
        let a = e.obfuscate("DE89 3704 0044 0532 0130 00").replace(' ', "");
        let b = e.obfuscate("DE89370400440532013000");
        assert_eq!(a, b);
        let c = e.obfuscate("de89370400440532013000");
        assert_eq!(c.to_uppercase(), b);
    }

    #[test]
    fn rejects_non_ibans() {
        let e = engine();
        for s in [
            "DE89 3704 0044 0532 0130 01",
            "DE8937040044053201300",
            "XX89370400440532013000",
            "De89370400440532013000",
            "xDE89370400440532013000",
            "DE89370400440532013000x",
            "DE89 3704 0044 0532 0130 000",
            "DE89  3704 0044 0532 0130 00",
        ] {
            assert!(e.analyze(s).is_empty(), "{s}");
        }
    }

    #[test]
    fn iban_followed_by_words() {
        let e = engine();
        let f = e.analyze("pay DE89 3704 0044 0532 0130 00 now, NL91 ABNA 0417 1643 00.");
        assert_eq!(f.len(), 2);
        assert_eq!(f[0].original, "DE89 3704 0044 0532 0130 00");
        assert_eq!(f[1].original, "NL91 ABNA 0417 1643 00");
    }

    #[test]
    fn registry_lengths_are_sane() {
        assert_eq!(expected_len("DE"), Some(22));
        assert_eq!(expected_len("NO"), Some(15));
        assert_eq!(expected_len("HN"), Some(28));
        assert_eq!(expected_len("XX"), None);
        assert!(LENGTHS.iter().all(|&(_, l)| (15..=MAX_LEN).contains(&l)));
    }

    proptest! {
        #[test]
        fn replacement_is_valid(
            cc in 0usize..LENGTHS.len(),
            body in proptest::collection::vec(0u8..36, 31),
            lower in any::<bool>(),
            spaced in any::<bool>(),
        ) {
            let (code, len) = LENGTHS[cc];
            let mut s: Vec<u8> = code.bytes().collect();
            s.extend_from_slice(b"00");
            for &v in body.iter().take(len - 4) {
                s.push(if v < 10 { b'0' + v } else { b'A' + v - 10 });
            }
            let check = 98 - mod97(&s);
            s[2] = b'0' + (check / 10) as u8;
            s[3] = b'0' + (check % 10) as u8;
            prop_assert!(valid_canonical(&s));
            let mut text = String::from_utf8(s.clone()).unwrap();
            if lower { text = text.to_lowercase(); }
            if spaced {
                let mut g = String::new();
                for (n, ch) in text.chars().enumerate() {
                    if n > 0 && n % 4 == 0 {
                        g.push(' ');
                    }
                    g.push(ch);
                }
                text = g;
            }
            let f = engine().analyze(&text);
            prop_assert_eq!(f.len(), 1);
            prop_assert_eq!(&f[0].original, &text);
            let r = &f[0].replacement;
            prop_assert!(valid_canonical(&canonical(r)));
            prop_assert_eq!(r.len(), text.len());
            prop_assert_eq!(&r[..2], &text[..2]);
            for (a, b) in text.chars().zip(r.chars()) {
                prop_assert_eq!(a == ' ', b == ' ');
                prop_assert_eq!(a.is_ascii_digit(), b.is_ascii_digit());
                prop_assert_eq!(a.is_ascii_lowercase(), b.is_ascii_lowercase());
                prop_assert_eq!(a.is_ascii_uppercase(), b.is_ascii_uppercase());
            }
        }
    }
}
