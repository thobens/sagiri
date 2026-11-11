//! Golden vectors for the pseudonymization primitives. Test key: bytes 0x00..=0x1f.
//!
//! Changing any expected value here is a breaking change: every user's replacements change.

use sagiri_core::key::Key;
use sagiri_core::pseudo::Pseudonymizer;

fn test_pseudo() -> Pseudonymizer {
    let mut b = [0u8; 32];
    for (i, x) in b.iter_mut().enumerate() {
        *x = i as u8;
    }
    Pseudonymizer::new(&Key::from_bytes(b))
}

#[test]
fn stream_vectors() {
    let p = test_pseudo();
    let cases: [(&str, &[u8], [u8; 16]); 3] = [
        (
            "ip",
            b"192.0.2.1",
            [
                0xbb, 0x0e, 0x1c, 0x32, 0xf8, 0xa6, 0x60, 0x6c, 0xb1, 0xfd, 0xe9, 0xa1, 0x27, 0x33,
                0xb9, 0xd1,
            ],
        ),
        (
            "host",
            b"git.acme.com",
            [
                0xa0, 0x7e, 0xfc, 0xed, 0x5b, 0x56, 0xd9, 0x78, 0x83, 0xdf, 0x1c, 0x2f, 0x0b, 0x7d,
                0x6e, 0xa0,
            ],
        ),
        (
            "x",
            b"",
            [
                0x81, 0x04, 0x9a, 0xa7, 0x17, 0x5c, 0xab, 0xa4, 0x54, 0x73, 0xda, 0xe0, 0xf0, 0x92,
                0xd9, 0x66,
            ],
        ),
    ];
    for (domain, input, expected) in cases {
        let mut got = [0u8; 16];
        p.stream(domain, input).fill(&mut got);
        assert_eq!(got, expected, "stream {domain}");
    }
}

#[test]
fn permute_vectors() {
    let p = test_pseudo();
    let cases: [(&str, u32, u64, u64); 5] = [
        ("ip4", 32, 3_221_225_985, 1_423_900_168),
        ("ip4", 32, 0, 1_112_891_488),
        ("card", 11, 1234, 1534),
        ("odd", 33, 4_294_967_296, 8_366_512_602),
        ("small", 2, 3, 0),
    ];
    for (domain, bits, value, expected) in cases {
        assert_eq!(p.permute(domain, bits, value), expected, "{domain} {value}");
    }
}

#[test]
fn mimic_vectors() {
    let p = test_pseudo();
    let cases: [(&str, &[u8], &str, &str); 5] = [
        ("term", b"acme", "acme", "fome"),
        ("term", b"acme", "ACME", "FOME"),
        (
            "term",
            b"Project Phoenix",
            "Project Phoenix",
            "Lapasep Famazor",
        ),
        ("num", b"x", "12-34 AB9", "01-53 RA6"),
        ("uni", b"u", "Zoë 7", "Ket 8"),
    ];
    for (domain, seed, shape, expected) in cases {
        assert_eq!(p.mimic(domain, seed, shape), expected, "{shape}");
    }
}

/// The first attempt for seed "7" yields "7" itself, so this vector locks the retry encoding
/// (seed followed by the big-endian 32-bit retry counter).
#[test]
fn mimic_retry_vector() {
    let p = test_pseudo();
    assert_eq!(p.mimic("d", b"7", "7"), "4");
}

/// The first attempt for seed "196" yields "42" itself, so the result is the retry output.
/// Unlike a one-digit shape, two digits distinguish the counter encodings.
#[test]
fn mimic_retry_vector_two_digits() {
    let p = test_pseudo();
    assert_eq!(p.mimic("d", b"196", "42"), "40");
}

#[test]
fn mimic_never_returns_the_shape() {
    let p = test_pseudo();
    for shape in ["7", "a", "A", "0", "z9"] {
        for i in 0..500 {
            let seed = i.to_string();
            assert_ne!(
                p.mimic("d", seed.as_bytes(), shape),
                shape,
                "{shape} {seed}"
            );
        }
    }
}
