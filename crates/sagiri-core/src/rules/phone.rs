//! Phone number rule: international form only (`+`, valid country code, 8-15 digits).

use std::ops::Range;

use crate::rule::{Context, Kind, Rule};

const MIN_DIGITS: usize = 8;
const MAX_DIGITS: usize = 15;

/// Assigned ITU-T E.164 country calling codes with one digit.
const CC1: &[u16] = &[1, 7];

/// Assigned codes with two digits.
const CC2: &[u16] = &[
    20, 27, 30, 31, 32, 33, 34, 36, 39, 40, 41, 43, 44, 45, 46, 47, 48, 49, 51, 52, 53, 54, 55, 56,
    57, 58, 60, 61, 62, 63, 64, 65, 66, 81, 82, 84, 86, 90, 91, 92, 93, 94, 95, 98,
];

/// Assigned codes with three digits.
const CC3: &[u16] = &[
    211, 212, 213, 216, 218, 220, 221, 222, 223, 224, 225, 226, 227, 228, 229, 230, 231, 232, 233,
    234, 235, 236, 237, 238, 239, 240, 241, 242, 243, 244, 245, 246, 247, 248, 249, 250, 251, 252,
    253, 254, 255, 256, 257, 258, 260, 261, 262, 263, 264, 265, 266, 267, 268, 269, 290, 291, 297,
    298, 299, 350, 351, 352, 353, 354, 355, 356, 357, 358, 359, 370, 371, 372, 373, 374, 375, 376,
    377, 378, 380, 381, 382, 383, 385, 386, 387, 389, 420, 421, 423, 500, 501, 502, 503, 504, 505,
    506, 507, 508, 509, 590, 591, 592, 593, 594, 595, 596, 597, 598, 599, 670, 672, 673, 674, 675,
    676, 677, 678, 679, 680, 681, 682, 683, 685, 686, 687, 688, 689, 690, 691, 692, 800, 808, 850,
    852, 853, 855, 856, 870, 878, 880, 881, 882, 883, 886, 888, 960, 961, 962, 963, 964, 965, 966,
    967, 968, 970, 971, 972, 973, 974, 975, 976, 977, 979, 992, 993, 994, 995, 996, 998,
];

/// Length of the country calling code at the start of `digits` (ASCII digits), if any.
fn country_code_len(digits: &[u8]) -> Option<usize> {
    let value = |n: usize| -> Option<u16> {
        (digits.len() >= n).then(|| {
            digits[..n]
                .iter()
                .fold(0, |a, &b| a * 10 + u16::from(b - b'0'))
        })
    };
    if value(1).is_some_and(|v| CC1.contains(&v)) {
        return Some(1);
    }
    if value(2).is_some_and(|v| CC2.contains(&v)) {
        return Some(2);
    }
    if value(3).is_some_and(|v| CC3.contains(&v)) {
        return Some(3);
    }
    None
}

/// A parsed phone number starting at a `+`.
struct Parsed {
    /// Byte offset one past the last digit.
    end: usize,
    /// All counted digits, country code first. The digit of a `(0)` group is not in here.
    digits: Vec<u8>,
    cc_len: usize,
    /// Byte range of a `(0)` trunk-prefix group directly after the country code.
    zero_group: Option<Range<usize>>,
}

fn digit_run_end(b: &[u8], from: usize) -> usize {
    let mut p = from;
    while p < b.len() && b[p].is_ascii_digit() {
        p += 1;
    }
    p
}

/// Parses a number at `start` (which holds `+`). The result is validated: country code,
/// digit count. The context check (what precedes the `+`) is the caller's job.
fn parse(text: &str, start: usize) -> Option<Parsed> {
    let b = text.as_bytes();
    let first_end = digit_run_end(b, start + 1);
    let cc_len = country_code_len(&b[start + 1..first_end])?;
    let mut digits: Vec<u8> = b[start + 1..first_end].to_vec();
    let mut zero_group = None;
    // Every group boundary: (end offset, digits so far, trunk group so far).
    let mut ends = vec![(first_end, digits.len(), None::<Range<usize>>)];
    let mut pos = first_end;
    let mut groups = 0;
    loop {
        let q = if matches!(b.get(pos), Some(b' ' | b'-' | b'.')) {
            pos + 1
        } else {
            pos
        };
        match b.get(q) {
            Some(b'(') => {
                let ge = digit_run_end(b, q + 1);
                if ge == q + 1 || b.get(ge) != Some(&b')') {
                    break;
                }
                pos = ge + 1;
                if groups == 0 && first_end - (start + 1) == cc_len && &b[q + 1..ge] == b"0" {
                    // The trunk group is not a boundary: it holds no counted digit.
                    zero_group = Some(q..ge + 1);
                } else {
                    digits.extend_from_slice(&b[q + 1..ge]);
                    ends.push((pos, digits.len(), zero_group.clone()));
                }
            }
            Some(c) if c.is_ascii_digit() => {
                let ge = digit_run_end(b, q);
                digits.extend_from_slice(&b[q..ge]);
                pos = ge;
                ends.push((pos, digits.len(), zero_group.clone()));
            }
            _ => break,
        }
        groups += 1;
    }
    // The longest prefix that ends at a group boundary and has a valid digit count. A
    // single digit run over the maximum has no shorter boundary and is rejected.
    let (end, n, zero_group) = ends
        .into_iter()
        .rev()
        .find(|(_, n, _)| (MIN_DIGITS..=MAX_DIGITS).contains(n))?;
    digits.truncate(n);
    Some(Parsed {
        end,
        digits,
        cc_len,
        zero_group,
    })
}

/// Astronomically unlikely, but never return the original digits: if `out` equals
/// `original`, shift the first national digit within 2-9.
fn avoid_identity(out: &mut [u8], original: &[u8], cc_len: usize) {
    if out == original {
        out[cc_len] = b'2' + (out[cc_len] - b'2' + 1) % 8;
    }
}

/// Detects and replaces international phone numbers.
pub struct PhoneRule;

impl Rule for PhoneRule {
    fn kind(&self) -> Kind {
        Kind::Phone
    }

    fn detect(&self, text: &str) -> Vec<Range<usize>> {
        let mut out: Vec<Range<usize>> = Vec::new();
        for (i, _) in text.match_indices('+') {
            let blocked = text[..i]
                .chars()
                .next_back()
                .is_some_and(|c| c.is_alphanumeric() || c == '+');
            if blocked {
                continue;
            }
            if let Some(p) = parse(text, i) {
                out.push(i..p.end);
            }
        }
        out
    }

    fn replace(&self, original: &str, ctx: &Context<'_>) -> String {
        let Some(p) = parse(original, 0) else {
            return original.to_string();
        };
        let mut canonical = vec![b'+'];
        canonical.extend_from_slice(&p.digits);
        let mut stream = ctx.pseudo().stream("phone", &canonical);
        let mut out = p.digits.clone();
        for (k, slot) in out.iter_mut().enumerate().skip(p.cc_len) {
            *slot = if k == p.cc_len {
                b'2' + stream.below(8) as u8
            } else {
                b'0' + stream.below(10) as u8
            };
        }
        avoid_identity(&mut out, &p.digits, p.cc_len);
        let mut it = out.into_iter();
        let mut s = String::with_capacity(original.len());
        for (i, c) in original.char_indices() {
            let in_zero = p.zero_group.as_ref().is_some_and(|r| r.contains(&i));
            if c.is_ascii_digit() && !in_zero {
                s.push(it.next().map_or(c, char::from));
            } else {
                s.push(c);
            }
        }
        s
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
            vec![Box::new(PhoneRule)],
        )
    }

    fn digits(s: &str) -> String {
        s.chars().filter(char::is_ascii_digit).collect()
    }

    /// The string with every digit blanked, to compare separators and parentheses.
    fn shape(s: &str) -> String {
        s.chars()
            .map(|c| if c.is_ascii_digit() { '#' } else { c })
            .collect()
    }

    #[test]
    fn detects_and_replaces() {
        let e = engine();
        let cases: [(&str, &str, usize); 11] = [
            ("+353 87 123 4567", "+353 87 123 4567", 3),
            ("+420 601 123 456", "+420 601 123 456", 3),
            ("+41 79 123 45 67", "+41 79 123 45 67", 2),
            ("+41791234567", "+41791234567", 2),
            ("+41 (0)79 123 45 67", "+41 (0)79 123 45 67", 2),
            ("+1 (415) 555-0132", "+1 (415) 555-0132", 1),
            ("+1-415-555-0132", "+1-415-555-0132", 1),
            ("+44 20 7946 0958", "+44 20 7946 0958", 2),
            ("+49.30.901820", "+49.30.901820", 2),
            ("Tel: +41 79 123 45 67.", "+41 79 123 45 67", 2),
            ("(+41 79 123 45 67)", "+41 79 123 45 67", 2),
        ];
        for (input, number, cc) in cases {
            let f = e.analyze(input);
            assert_eq!(f.len(), 1, "{input}");
            assert_eq!(f[0].original, number, "{input}");
            let r = &f[0].replacement;
            assert_eq!(shape(r), shape(number), "{input}");
            assert_ne!(r, number, "{input}");
            // The digit of a `(0)` group is neither counted nor replaced.
            let d_in = digits(&number.replace("(0)", ""));
            let d_out = digits(&r.replace("(0)", ""));
            assert_eq!(d_in.len(), d_out.len(), "{input}");
            assert_eq!(d_in[..cc], d_out[..cc], "{input}");
            assert!((b'2'..=b'9').contains(&d_out.as_bytes()[cc]), "{input}");
            assert_eq!(e.obfuscate(input), input.replace(number, r), "{input}");
        }
    }

    #[test]
    fn trunk_group_kept_and_digits_equal() {
        let e = engine();
        let a = e.obfuscate("+41 79 123 45 67");
        let b = e.obfuscate("+41791234567");
        let c = e.obfuscate("+41 (0)79 123 45 67");
        assert_eq!(digits(&a), digits(&b));
        assert!(c.starts_with("+41 (0)"), "{c}");
        assert_eq!(c.replacen("(0)", "", 1), a);
        assert_ne!(c, a);
    }

    #[test]
    fn first_national_digit_is_2_to_9() {
        let e = engine();
        for (cc, ndigits) in [("1", 10), ("41", 9), ("353", 9)] {
            for n in 0..300u32 {
                let national = 1_000_000_000 + n * 7919;
                let input = format!("+{cc} {}", &national.to_string()[..ndigits]);
                let out = e.obfuscate(&input);
                let d = digits(&out);
                assert!((b'2'..=b'9').contains(&d.as_bytes()[cc.len()]), "{out}");
                assert_eq!(d.len(), cc.len() + ndigits, "{out}");
                assert!(d.starts_with(cc), "{out}");
            }
        }
    }

    #[test]
    fn not_detected() {
        let e = engine();
        for s in [
            "+1",
            "+999 12 345 67 89",
            "a+41791234567",
            "2+41791234567",
            "++41791234567",
            "079 123 45 67",
            "0041 79 123 45 67",
            "+4179123456789012",
            "+41  79 123 45 67",
            "+4 1791234567",
            "+41 7912",
        ] {
            assert_eq!(e.obfuscate(s), s, "{s}");
            assert!(PhoneRule.detect(s).is_empty(), "{s}");
        }
    }

    #[test]
    fn trailing_punctuation_not_part_of_match() {
        let t = "Call +41 79 123 45 67. Or (+1 415 555 0132), or +1-415-555-0132-";
        let r = PhoneRule.detect(t);
        let got: Vec<&str> = r.iter().map(|r| &t[r.clone()]).collect();
        assert_eq!(
            got,
            ["+41 79 123 45 67", "+1 415 555 0132", "+1-415-555-0132"]
        );
    }

    #[test]
    fn trailing_parenthesised_group_is_part_of_the_match() {
        let t = "+41 79 123 45 (67)";
        assert_eq!(PhoneRule.detect(t), vec![0..t.len()]);
        let e = engine();
        let out = e.obfuscate(t);
        assert_eq!(shape(&out), shape(t));
        assert_ne!(out, t);
    }

    #[test]
    fn too_long_grouped_run_falls_back_to_longest_valid_prefix() {
        let t = "+41 79 123 45 67 89 01 23";
        let r = PhoneRule.detect(t);
        assert_eq!(r.len(), 1);
        assert_eq!(&t[r[0].clone()], "+41 79 123 45 67 89 01");
        let t = "+41 79 123 45 67 12345";
        let r = PhoneRule.detect(t);
        assert_eq!(&t[r[0].clone()], "+41 79 123 45 67");
        // A contiguous run over 15 digits stays rejected.
        assert!(PhoneRule.detect("+4179123456789012").is_empty());
    }

    #[test]
    fn known_limitation_following_number_is_absorbed() {
        // `24` joins the number as one more group.
        let t = "Call +1 415 555 0132 24 hours";
        let r = PhoneRule.detect(t);
        assert_eq!(&t[r[0].clone()], "+1 415 555 0132 24");
    }

    #[test]
    fn avoid_identity_changes_a_colliding_result() {
        let original = *b"41791234567";
        let mut out = original;
        avoid_identity(&mut out, &original, 2);
        assert_ne!(out, original);
        assert_eq!(&out[..2], b"41");
        assert!((b'2'..=b'9').contains(&out[2]));
        assert_eq!(&out[3..], &original[3..]);
        // Wraps from 9 to 2.
        let original = *b"41991234567";
        let mut out = original;
        avoid_identity(&mut out, &original, 2);
        assert_eq!(out[2], b'2');
        // A differing result is left alone.
        let mut other = *b"41701234567";
        avoid_identity(&mut other, &original, 2);
        assert_eq!(&other, b"41701234567");
    }

    #[test]
    fn same_number_same_replacement() {
        let e = engine();
        let out = e.obfuscate("+41 79 123 45 67 and +41 79 123 45 67");
        let (a, b) = out.split_once(" and ").unwrap();
        assert_eq!(a, b);
        assert_eq!(e.obfuscate("+41 79 123 45 67"), a);
        assert_eq!(engine().obfuscate("+41 79 123 45 67"), a);
    }

    #[test]
    fn different_numbers_differ() {
        let e = engine();
        assert_ne!(
            digits(&e.obfuscate("+41 79 123 45 67")),
            digits(&e.obfuscate("+41 79 123 45 68"))
        );
    }

    #[test]
    fn country_code_table_is_prefix_free() {
        for &c in CC3 {
            assert!(!CC2.contains(&(c / 10)) && !CC1.contains(&(c / 100)), "{c}");
        }
        for &c in CC2 {
            assert!(!CC1.contains(&(c / 10)), "{c}");
        }
    }
}
