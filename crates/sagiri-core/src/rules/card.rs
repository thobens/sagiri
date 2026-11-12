//! Payment card rule: Luhn-valid numbers with a recognized brand prefix and length.

use std::ops::Range;

use crate::rule::{Context, Kind, Rule};

const MIN_DIGITS: usize = 13;
const MAX_DIGITS: usize = 19;

fn num(d: &[u8], len: usize) -> u32 {
    d[..len]
        .iter()
        .fold(0, |a, &b| a * 10 + u32::from(b - b'0'))
}

/// Length of the brand prefix if `d` (ASCII digits) has a recognized brand prefix and a
/// length valid for that brand.
fn brand_prefix_len(d: &[u8]) -> Option<usize> {
    let n = d.len();
    if !(MIN_DIGITS..=MAX_DIGITS).contains(&n) {
        return None;
    }
    let p2 = num(d, 2);
    let p3 = num(d, 3);
    let p4 = num(d, 4);
    // Visa
    if d[0] == b'4' && matches!(n, 13 | 16 | 19) {
        return Some(1);
    }
    // Mastercard
    if n == 16 {
        if (51..=55).contains(&p2) {
            return Some(2);
        }
        if (2221..=2720).contains(&p4) {
            return Some(4);
        }
    }
    // American Express
    if n == 15 && (p2 == 34 || p2 == 37) {
        return Some(2);
    }
    // Discover
    if n >= 16 {
        if p4 == 6011 {
            return Some(4);
        }
        if (644..=649).contains(&p3) {
            return Some(3);
        }
        if p2 == 65 {
            return Some(2);
        }
    }
    // Diners Club
    if n >= 14 {
        if (300..=305).contains(&p3) {
            return Some(3);
        }
        if matches!(p2, 36 | 38 | 39) {
            return Some(2);
        }
    }
    // JCB
    if n >= 16 && (3528..=3589).contains(&p4) {
        return Some(4);
    }
    // UnionPay
    if n >= 16 && p2 == 62 {
        return Some(2);
    }
    // Maestro
    if matches!(
        p4,
        5018 | 5020 | 5038 | 5893 | 6304 | 6759 | 6761 | 6762 | 6763
    ) {
        return Some(4);
    }
    None
}

/// Luhn sum over `d`; with `skip_last` the last digit is left out of the sum.
fn luhn_sum(d: &[u8], skip_last: bool) -> u32 {
    let mut sum = 0;
    for (i, &b) in d.iter().rev().enumerate().skip(usize::from(skip_last)) {
        let mut v = u32::from(b - b'0');
        if i % 2 == 1 {
            v *= 2;
            if v > 9 {
                v -= 9;
            }
        }
        sum += v;
    }
    sum
}

fn luhn_valid(d: &[u8]) -> bool {
    luhn_sum(d, false).is_multiple_of(10)
}

fn luhn_check_digit(d_without_last: &[u8]) -> u8 {
    // Append a placeholder so positions line up, then skip it.
    let mut v = d_without_last.to_vec();
    v.push(b'0');
    b'0' + ((10 - luhn_sum(&v, true) % 10) % 10) as u8
}

/// Group end offsets starting at `start`, with cumulative digit counts. Groups are
/// separated by single spaces or single hyphens, one style per number.
fn groups_from(text: &str, start: usize) -> Vec<(usize, usize)> {
    let b = text.as_bytes();
    let mut out = Vec::new();
    let mut pos = start;
    let mut digits = 0;
    let mut sep: Option<u8> = None;
    loop {
        let gs = pos;
        while pos < b.len() && b[pos].is_ascii_digit() {
            pos += 1;
        }
        digits += pos - gs;
        if digits > MAX_DIGITS {
            break;
        }
        out.push((pos, digits));
        if pos + 1 < b.len()
            && (b[pos] == b' ' || b[pos] == b'-')
            && sep.is_none_or(|s| s == b[pos])
            && b[pos + 1].is_ascii_digit()
        {
            sep = Some(b[pos]);
            pos += 1;
        } else {
            break;
        }
    }
    out
}

/// Detects and replaces payment card numbers.
pub struct PaymentCardRule;

impl Rule for PaymentCardRule {
    fn kind(&self) -> Kind {
        Kind::PaymentCard
    }

    fn detect(&self, text: &str) -> Vec<Range<usize>> {
        let b = text.as_bytes();
        let mut out = Vec::new();
        let mut i = 0;
        while i < b.len() {
            let at_start = b[i].is_ascii_digit()
                && text[..i]
                    .chars()
                    .next_back()
                    .is_none_or(|c| !c.is_alphanumeric());
            if !at_start {
                i += 1;
                continue;
            }
            // Not the fraction of a decimal number: `<digit>.` or `<digit>,` before.
            let decimal_before =
                i >= 2 && matches!(b[i - 1], b'.' | b',') && b[i - 2].is_ascii_digit();
            let mut found = None;
            if !decimal_before {
                for &(end, digits) in groups_from(text, i).iter().rev() {
                    if digits < MIN_DIGITS {
                        break;
                    }
                    if text[end..]
                        .chars()
                        .next()
                        .is_some_and(char::is_alphanumeric)
                    {
                        continue;
                    }
                    if matches!(b.get(end), Some(b'.' | b','))
                        && b.get(end + 1).is_some_and(u8::is_ascii_digit)
                    {
                        continue;
                    }
                    let d: Vec<u8> = text[i..end].bytes().filter(u8::is_ascii_digit).collect();
                    if luhn_valid(&d) && brand_prefix_len(&d).is_some() {
                        found = Some(end);
                        break;
                    }
                }
            }
            match found {
                Some(end) => {
                    out.push(i..end);
                    i = end;
                }
                None => {
                    // Skip the rest of this digit run.
                    while i < b.len() && b[i].is_ascii_digit() {
                        i += 1;
                    }
                }
            }
        }
        out
    }

    fn replace(&self, original: &str, ctx: &Context<'_>) -> String {
        let digits: Vec<u8> = original.bytes().filter(u8::is_ascii_digit).collect();
        let Some(prefix) = brand_prefix_len(&digits) else {
            return original.to_string();
        };
        let n = digits.len();
        let mut out = digits.clone();
        let mut stream = ctx.pseudo().stream("card", &digits);
        for slot in &mut out[prefix..n - 1] {
            *slot = b'0' + stream.below(10) as u8;
        }
        if out[..n - 1] == digits[..n - 1] {
            // Astronomically unlikely, but never return the original digits.
            out[prefix] = b'0' + (out[prefix] - b'0' + 1) % 10;
        }
        out[n - 1] = luhn_check_digit(&out[..n - 1]);
        let mut it = out.into_iter();
        original
            .chars()
            .map(|c| {
                if c.is_ascii_digit() {
                    it.next().map_or(c, char::from)
                } else {
                    c
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
            vec![Box::new(PaymentCardRule)],
        )
    }

    fn digits(s: &str) -> Vec<u8> {
        s.bytes().filter(u8::is_ascii_digit).collect()
    }

    #[test]
    fn detects_and_replaces_valid_cards() {
        let e = engine();
        for s in [
            "4111 1111 1111 1111",
            "4111-1111-1111-1111",
            "4111111111111111",
            "5555555555554444",
            "378282246310005",
            "3782 822463 10005",
            "6011111111111117",
            "30569309025904",
            "3530111333300000",
        ] {
            let text = format!("card {s} end");
            let f = e.analyze(&text);
            assert_eq!(f.len(), 1, "{s}");
            assert_eq!(f[0].original, s);
            let r = &f[0].replacement;
            assert_eq!(r.len(), s.len(), "{s}");
            assert!(luhn_valid(&digits(r)), "{s} -> {r}");
            let pl = brand_prefix_len(&digits(s)).unwrap();
            assert_eq!(digits(r)[..pl], digits(s)[..pl], "{s}");
            for (a, b) in s.chars().zip(r.chars()) {
                assert_eq!(a.is_ascii_digit(), b.is_ascii_digit());
                if !a.is_ascii_digit() {
                    assert_eq!(a, b);
                }
            }
            assert_ne!(digits(r), digits(s));
        }
    }

    #[test]
    fn same_digits_regardless_of_separators() {
        let e = engine();
        let a = digits(&e.obfuscate("4111 1111 1111 1111"));
        let b = digits(&e.obfuscate("4111-1111-1111-1111"));
        let c = digits(&e.obfuscate("4111111111111111"));
        assert_eq!(a, b);
        assert_eq!(a, c);
    }

    #[test]
    fn rejects_non_cards() {
        let e = engine();
        for s in [
            "4111 1111 1111 1112",
            "1234567890123456",
            "4111  1111 1111 1111",
            "12.4111111111111111",
            "12,4111111111111111",
            "4111111111111111.5",
            "A4111111111111111",
            "41111111111111111111",
            "1699999999999",
            "4111 1111-1111 1111",
        ] {
            assert!(e.analyze(s).is_empty(), "{s}");
        }
    }

    #[test]
    fn card_followed_by_more_text() {
        let e = engine();
        let f = e.analyze("4111 1111 1111 1111 12/25.");
        assert_eq!(f.len(), 1);
        assert_eq!(f[0].original, "4111 1111 1111 1111");
        let f = e.analyze("4111111111111111.");
        assert_eq!(f.len(), 1);
    }

    #[test]
    fn brand_table() {
        let ok = |s: &str| brand_prefix_len(s.as_bytes()).is_some();
        assert!(ok("2221000000000009"));
        assert!(!ok("2721000000000000"));
        assert!(!ok("222100000000000"));
        assert!(ok("6440000000000000"));
        assert!(ok("36000000000000"));
        assert!(!ok("37000000000000"));
        assert!(ok("6200000000000000000"));
        assert!(ok("5018000000000"));
        assert!(!ok("4111111111111111111111"));
    }

    const BRANDS: [(&str, usize); 14] = [
        ("4", 16),
        ("4", 13),
        ("4", 19),
        ("51", 16),
        ("2221", 16),
        ("34", 15),
        ("37", 15),
        ("6011", 16),
        ("65", 19),
        ("300", 14),
        ("36", 16),
        ("3528", 16),
        ("62", 17),
        ("5018", 13),
    ];

    proptest! {
        #[test]
        fn replacement_is_valid_and_differs(
            idx in 0usize..14,
            seed in proptest::collection::vec(0u8..10, 19),
        ) {
            let (prefix, len) = BRANDS[idx];
            let mut d: Vec<u8> = prefix.bytes().collect();
            let mut k = 0;
            while d.len() < len - 1 {
                d.push(b'0' + seed[k]);
                k += 1;
            }
            let c = luhn_check_digit(&d);
            d.push(c);
            prop_assert!(luhn_valid(&d));
            prop_assert!(brand_prefix_len(&d).is_some());
            let s = String::from_utf8(d.clone()).unwrap();
            let f = engine().analyze(&s);
            prop_assert_eq!(f.len(), 1);
            let r = digits(&f[0].replacement);
            prop_assert_eq!(r.len(), d.len());
            prop_assert!(luhn_valid(&r));
            prop_assert_ne!(r, d);
        }
    }
}
