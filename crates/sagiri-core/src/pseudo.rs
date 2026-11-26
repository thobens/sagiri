//! Deterministic, keyed building blocks for replacements.
//!
//! Every derivation is HMAC-SHA256 under the scheme label `sagiri/v1`. No `rand` crate.
//! The output is a compatibility contract locked by golden vectors.
//!
//! Every function takes a `domain` label that separates the uses of the key: the same input
//! under two domains gives unrelated outputs. For a fixed key, domain and input the output
//! is the same on every call and in every future version.

use hmac::{Hmac, KeyInit, Mac};
use sha2::Sha256;
use zeroize::Zeroizing;

use crate::key::Key;

type HmacSha256 = Hmac<Sha256>;

const SCHEME: &[u8] = b"sagiri/v1";
const ROUNDS: u8 = 10;
const CONSONANTS: &[u8] = b"bdfgklmnprstvz";
const VOWELS: &[u8] = b"aeiou";

/// Source of all derived values for one key.
///
/// All randomness of Sagiri comes from here. Rules reach it through
/// [`Context::pseudo`](crate::rule::Context::pseudo).
pub struct Pseudonymizer {
    key: Zeroizing<[u8; 32]>,
}

impl Pseudonymizer {
    /// Copies the key; the copy is zeroized on drop.
    ///
    /// ```
    /// use sagiri_core::key::Key;
    /// use sagiri_core::pseudo::Pseudonymizer;
    ///
    /// let key = Key::from_bytes(std::array::from_fn(|i| i as u8));
    /// let p = Pseudonymizer::new(&key);
    /// drop(key); // the pseudonymizer keeps its own copy
    /// assert_eq!(p.mimic("doc", b"seed", "abc"), p.mimic("doc", b"seed", "abc"));
    /// ```
    pub fn new(key: &Key) -> Self {
        Pseudonymizer {
            key: Zeroizing::new(*key.as_bytes()),
        }
    }

    /// `HMAC(key, "sagiri/v1" 0 domain 0)`, ready for more input.
    fn domain_mac(&self, domain: &str) -> HmacSha256 {
        assert!(!domain.as_bytes().contains(&0), "domain contains NUL");
        let mut m = HmacSha256::new_from_slice(&self.key[..]).expect("HMAC accepts any key length");
        m.update(SCHEME);
        m.update(&[0]);
        m.update(domain.as_bytes());
        m.update(&[0]);
        m
    }

    /// Deterministic byte stream for (domain, input).
    ///
    /// The stream is indistinguishable from random to anyone without the key, and is the
    /// same for the same key, domain and input.
    ///
    /// # Panics
    ///
    /// If `domain` contains a NUL byte.
    ///
    /// ```
    /// # use sagiri_core::{key::Key, pseudo::Pseudonymizer};
    /// # let p = Pseudonymizer::new(&Key::from_bytes(std::array::from_fn(|i| i as u8)));
    /// let mut a = [0u8; 16];
    /// let mut b = [0u8; 16];
    /// p.stream("doc", b"192.0.2.1").fill(&mut a);
    /// p.stream("doc", b"192.0.2.1").fill(&mut b);
    /// assert_eq!(a, b);
    ///
    /// p.stream("other", b"192.0.2.1").fill(&mut b);
    /// assert_ne!(a, b);
    /// ```
    pub fn stream(&self, domain: &str, input: &[u8]) -> DetStream {
        let mut base = self.domain_mac(domain);
        base.update(input);
        DetStream {
            base,
            counter: 0,
            block: [0; 32],
            pos: 32,
        }
    }

    fn feistel_round_fn(&self, domain: &str, round: u8, bits: u32, right: u64) -> u64 {
        let mut m = self.domain_mac(domain);
        m.update(b"feistel");
        m.update(&[round, bits as u8]);
        m.update(&right.to_be_bytes());
        let out = m.finalize().into_bytes();
        let first = u64::from_be_bytes(out[..8].try_into().expect("8 bytes"));
        first >> (64 - bits / 2)
    }

    fn feistel(&self, domain: &str, bits: u32, value: u64) -> u64 {
        debug_assert!(bits & 1 == 0 && (2..=64).contains(&bits));
        let half = bits / 2;
        let mask = (1u64 << half) - 1;
        let mut l = value >> half;
        let mut r = value & mask;
        for round in 0..ROUNDS {
            let f = self.feistel_round_fn(domain, round, bits, r);
            (l, r) = (r, l ^ f);
        }
        l << half | r
    }

    /// Keyed bijection on `0..2^bits`, for `2 <= bits <= 64`.
    ///
    /// Distinct values always map to distinct values, so two originals never share a
    /// replacement.
    ///
    /// # Panics
    ///
    /// If `bits` is out of range, `value >= 2^bits` or `domain` contains a NUL byte.
    ///
    /// ```
    /// # use sagiri_core::{key::Key, pseudo::Pseudonymizer};
    /// # let p = Pseudonymizer::new(&Key::from_bytes(std::array::from_fn(|i| i as u8)));
    /// // 192.0.2.1 as a number, a golden vector of the test key.
    /// assert_eq!(p.permute("ip4", 32, 3_221_225_985), 1_423_900_168);
    ///
    /// let mut all: Vec<u64> = (0..16).map(|v| p.permute("doc", 4, v)).collect();
    /// all.sort();
    /// assert_eq!(all, (0..16).collect::<Vec<u64>>());
    /// ```
    pub fn permute(&self, domain: &str, bits: u32, value: u64) -> u64 {
        assert!((2..=64).contains(&bits), "bits out of range");
        assert!(bits == 64 || value >> bits == 0, "value out of range");
        if bits & 1 == 0 {
            return self.feistel(domain, bits, value);
        }
        let mut v = value;
        loop {
            v = self.feistel(domain, bits + 1, v);
            if v >> bits == 0 {
                return v;
            }
        }
    }

    /// Keyed bijection on the subset of `0..2^bits` for which `member` is true.
    ///
    /// The result is a member again, and distinct members map to distinct members. This is
    /// how a value stays in its category, a public address for example. The cost grows with
    /// the share of non-members, so the subset should be a large part of the range.
    ///
    /// # Panics
    ///
    /// If `member(value)` is false, or for the cases of [`permute`](Self::permute).
    ///
    /// ```
    /// # use sagiri_core::{key::Key, pseudo::Pseudonymizer};
    /// # let p = Pseudonymizer::new(&Key::from_bytes(std::array::from_fn(|i| i as u8)));
    /// let odd = |v: u64| v % 2 == 1;
    /// let out = p.permute_within("doc", 8, 37, odd);
    /// assert!(odd(out) && out < 256);
    /// assert_eq!(p.permute_within("doc", 8, 37, odd), out);
    /// ```
    pub fn permute_within(
        &self,
        domain: &str,
        bits: u32,
        value: u64,
        member: impl Fn(u64) -> bool,
    ) -> u64 {
        assert!(member(value), "value is not a member");
        let mut v = value;
        loop {
            v = self.permute(domain, bits, v);
            if member(v) {
                return v;
            }
        }
    }

    /// Pseudo-text with the same shape as `shape`, seeded by (domain, seed).
    ///
    /// Letters become pronounceable ASCII letters (case kept), digits become ASCII digits,
    /// everything else is copied. The result never equals `shape` if `shape` has a letter
    /// or digit.
    ///
    /// The output has as many characters as `shape`, so a replacement keeps the length and
    /// layout of its original. It contains a line break only where `shape` has one.
    ///
    /// # Panics
    ///
    /// If `domain` contains a NUL byte.
    ///
    /// ```
    /// # use sagiri_core::{key::Key, pseudo::Pseudonymizer};
    /// # let p = Pseudonymizer::new(&Key::from_bytes(std::array::from_fn(|i| i as u8)));
    /// // A golden vector of the test key.
    /// assert_eq!(p.mimic("num", b"x", "12-34 AB9"), "01-53 RA6");
    ///
    /// let out = p.mimic("doc", b"jane", "Jane Doe");
    /// assert_ne!(out, "Jane Doe");
    /// assert_eq!(out.len(), 8);
    /// assert_eq!(out.find(' '), Some(4));
    /// ```
    pub fn mimic(&self, domain: &str, seed: &[u8], shape: &str) -> String {
        let has_content = shape.chars().any(|c| c.is_alphabetic() || c.is_numeric());
        let mut retry: u32 = 0;
        loop {
            let out = if retry == 0 {
                self.mimic_once(domain, seed, shape)
            } else {
                let mut s = seed.to_vec();
                s.extend_from_slice(&retry.to_be_bytes());
                self.mimic_once(domain, &s, shape)
            };
            if !has_content || out != shape {
                return out;
            }
            retry += 1;
        }
    }

    fn mimic_once(&self, domain: &str, seed: &[u8], shape: &str) -> String {
        let mut st = self.stream(domain, seed);
        let mut out = String::with_capacity(shape.len());
        let mut vowel_next = false;
        let mut in_run = false;
        for c in shape.chars() {
            if c.is_alphabetic() {
                if !in_run {
                    vowel_next = st.below(2) == 1;
                    in_run = true;
                }
                let set = if vowel_next { VOWELS } else { CONSONANTS };
                let l = *st.pick(set) as char;
                vowel_next = !vowel_next;
                out.push(if c.is_uppercase() {
                    l.to_ascii_uppercase()
                } else {
                    l
                });
            } else {
                in_run = false;
                if c.is_numeric() {
                    out.push((b'0' + st.below(10) as u8) as char);
                } else {
                    out.push(c);
                }
            }
        }
        out
    }
}

/// Deterministic byte stream; see [`Pseudonymizer::stream`].
///
/// Each method consumes bytes from the stream, so the order of calls matters: the same
/// sequence of calls on the same stream gives the same results.
pub struct DetStream {
    base: HmacSha256,
    counter: u32,
    block: [u8; 32],
    pos: usize,
}

impl DetStream {
    /// Copies the next bytes of the stream into `buf`.
    ///
    /// # Panics
    ///
    /// When about 2^37 bytes (2^32 blocks of 32 bytes) have been read from one stream.
    pub fn fill(&mut self, buf: &mut [u8]) {
        let mut done = 0;
        while done < buf.len() {
            if self.pos == self.block.len() {
                let mut m = self.base.clone();
                m.update(&self.counter.to_be_bytes());
                self.counter = self.counter.checked_add(1).expect("stream exhausted");
                self.block = m.finalize().into_bytes().into();
                self.pos = 0;
            }
            let n = (buf.len() - done).min(self.block.len() - self.pos);
            buf[done..done + n].copy_from_slice(&self.block[self.pos..self.pos + n]);
            self.pos += n;
            done += n;
        }
    }

    /// Next 8 bytes, big-endian.
    pub fn next_u64(&mut self) -> u64 {
        let mut b = [0u8; 8];
        self.fill(&mut b);
        u64::from_be_bytes(b)
    }

    /// Uniform value in `0..n` (rejection sampling), without the bias of a plain modulo.
    ///
    /// # Panics
    ///
    /// If `n == 0`.
    ///
    /// ```
    /// # use sagiri_core::{key::Key, pseudo::Pseudonymizer};
    /// # let p = Pseudonymizer::new(&Key::from_bytes(std::array::from_fn(|i| i as u8)));
    /// let mut s = p.stream("doc", b"dice");
    /// let rolls: Vec<u64> = (0..5).map(|_| s.below(6) + 1).collect();
    /// assert!(rolls.iter().all(|r| (1..=6).contains(r)));
    ///
    /// let mut again = p.stream("doc", b"dice");
    /// let repeat: Vec<u64> = (0..5).map(|_| again.below(6) + 1).collect();
    /// assert_eq!(rolls, repeat);
    /// ```
    pub fn below(&mut self, n: u64) -> u64 {
        assert!(n > 0, "below(0)");
        let threshold = n.wrapping_neg() % n;
        loop {
            let x = self.next_u64();
            if x >= threshold {
                return x % n;
            }
        }
    }

    /// Uniformly chosen element.
    ///
    /// # Panics
    ///
    /// On an empty slice.
    ///
    /// ```
    /// # use sagiri_core::{key::Key, pseudo::Pseudonymizer};
    /// # let p = Pseudonymizer::new(&Key::from_bytes(std::array::from_fn(|i| i as u8)));
    /// let names = ["alpha", "bravo", "charlie"];
    /// let first = *p.stream("doc", b"jane").pick(&names);
    /// assert!(names.contains(&first));
    /// assert_eq!(*p.stream("doc", b"jane").pick(&names), first);
    /// ```
    pub fn pick<'a, T>(&mut self, items: &'a [T]) -> &'a T {
        &items[self.below(items.len() as u64) as usize]
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    fn test_key() -> Key {
        let mut b = [0u8; 32];
        for (i, x) in b.iter_mut().enumerate() {
            *x = i as u8;
        }
        Key::from_bytes(b)
    }

    fn ps() -> Pseudonymizer {
        Pseudonymizer::new(&test_key())
    }

    fn bytes(s: &mut DetStream, n: usize) -> Vec<u8> {
        let mut v = vec![0; n];
        s.fill(&mut v);
        v
    }

    #[test]
    fn stream_is_deterministic_and_separated() {
        let p = ps();
        let a = bytes(&mut p.stream("d", b"x"), 100);
        assert_eq!(a, bytes(&mut p.stream("d", b"x"), 100));
        assert_ne!(a, bytes(&mut p.stream("d", b"y"), 100));
        assert_ne!(a, bytes(&mut p.stream("e", b"x"), 100));
        let other = Pseudonymizer::new(&Key::from_bytes([7; 32]));
        assert_ne!(a, bytes(&mut other.stream("d", b"x"), 100));
        assert_ne!(
            bytes(&mut p.stream("a", b"b"), 32),
            bytes(&mut p.stream("ab", b""), 32)
        );
    }

    #[test]
    fn stream_pieces_equal_whole() {
        let p = ps();
        let whole = bytes(&mut p.stream("d", b"x"), 100);
        let mut s = p.stream("d", b"x");
        let mut pieced = Vec::new();
        for n in [1, 7, 24, 33, 3, 32] {
            pieced.extend(bytes(&mut s, n));
        }
        assert_eq!(pieced, whole);
    }

    #[test]
    fn stream_first_block_matches_spec() {
        let p = ps();
        let mut m = HmacSha256::new_from_slice(test_key().as_bytes()).unwrap();
        m.update(b"sagiri/v1\0dom\0inp");
        m.update(&0u32.to_be_bytes());
        let expect: [u8; 32] = m.finalize().into_bytes().into();
        assert_eq!(bytes(&mut p.stream("dom", b"inp"), 32), expect);
    }

    #[test]
    #[should_panic]
    fn nul_in_domain_panics() {
        ps().stream("a\0b", b"");
    }

    #[test]
    #[should_panic]
    fn below_zero_panics() {
        ps().stream("d", b"").below(0);
    }

    #[test]
    #[should_panic]
    fn pick_empty_panics() {
        let e: [u8; 0] = [];
        ps().stream("d", b"").pick(&e);
    }

    #[test]
    fn below_is_uniform_for_six() {
        let mut s = ps().stream("uniform", b"");
        let mut counts = [0u32; 6];
        for _ in 0..60_000 {
            counts[s.below(6) as usize] += 1;
        }
        for c in counts {
            assert!((9_500..=10_500).contains(&c), "{counts:?}");
        }
    }

    proptest! {
        #[test]
        fn below_stays_in_range(n in 1u64..=u64::MAX, seed in any::<Vec<u8>>()) {
            let mut s = ps().stream("prop", &seed);
            for _ in 0..8 {
                prop_assert!(s.below(n) < n);
            }
        }
    }

    #[test]
    fn permute_is_a_bijection() {
        let p = ps();
        for bits in [2u32, 3, 8, 11, 16] {
            let size = 1u64 << bits;
            let mut seen = vec![false; size as usize];
            for v in 0..size {
                let o = p.permute("perm", bits, v);
                assert!(o < size);
                assert!(!seen[o as usize], "collision at bits={bits}");
                seen[o as usize] = true;
            }
        }
    }

    #[test]
    fn permute_handles_wide_values() {
        let p = ps();
        for bits in [32u32, 33, 63, 64] {
            let max = if bits == 64 {
                u64::MAX
            } else {
                (1 << bits) - 1
            };
            for v in [0, 1, max / 2, max] {
                let o = p.permute("wide", bits, v);
                assert!(bits == 64 || o <= max);
                assert_eq!(o, p.permute("wide", bits, v));
            }
        }
    }

    #[test]
    #[should_panic]
    fn permute_rejects_out_of_range_value() {
        ps().permute("d", 8, 256);
    }

    #[test]
    #[should_panic]
    fn permute_rejects_one_bit() {
        ps().permute("d", 1, 0);
    }

    #[test]
    fn permute_within_is_a_bijection_on_the_subset() {
        let p = ps();
        let member = |v: u64| !v.is_multiple_of(3);
        let mut seen = vec![false; 1 << 16];
        let mut count = 0;
        for v in (0..1u64 << 16).filter(|v| member(*v)) {
            let o = p.permute_within("within", 16, v, member);
            assert!(member(o));
            assert!(!seen[o as usize]);
            seen[o as usize] = true;
            count += 1;
        }
        assert_eq!(count, seen.iter().filter(|s| **s).count());
    }

    fn class(c: char) -> u8 {
        if c.is_alphabetic() {
            0
        } else if c.is_numeric() {
            1
        } else {
            2
        }
    }

    proptest! {
        #[test]
        fn mimic_preserves_shape(shape in any::<String>(), seed in any::<Vec<u8>>()) {
            let out = ps().mimic("mimic", &seed, &shape);
            prop_assert_eq!(out.chars().count(), shape.chars().count());
            for (a, b) in shape.chars().zip(out.chars()) {
                match class(a) {
                    0 => {
                        prop_assert!(b.is_ascii_alphabetic());
                        prop_assert_eq!(a.is_uppercase(), b.is_ascii_uppercase());
                    }
                    1 => prop_assert!(b.is_ascii_digit()),
                    _ => prop_assert_eq!(a, b),
                }
            }
            if shape.chars().any(|c| c.is_alphabetic() || c.is_numeric()) {
                prop_assert_ne!(out, shape);
            }
        }

        #[test]
        fn mimic_is_case_independent(shape in "[a-z0-9 .@-]{0,30}", seed in any::<Vec<u8>>()) {
            let p = ps();
            prop_assert_eq!(
                p.mimic("case", &seed, &shape.to_uppercase()),
                p.mimic("case", &seed, &shape).to_uppercase()
            );
        }
    }

    #[test]
    fn mimic_alternates_consonants_and_vowels() {
        let out = ps().mimic("alt", b"s", "abcdefgh ijkl");
        let is_v = |c: char| "aeiou".contains(c);
        for w in out.split(' ') {
            let cs: Vec<char> = w.chars().collect();
            for p in cs.windows(2) {
                assert_ne!(is_v(p[0]), is_v(p[1]), "{out}");
            }
        }
    }

    #[test]
    fn mimic_without_letters_or_digits_copies_shape() {
        assert_eq!(ps().mimic("d", b"s", " .-@"), " .-@");
        assert_eq!(ps().mimic("d", b"s", ""), "");
    }
}
