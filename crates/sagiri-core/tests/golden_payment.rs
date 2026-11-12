//! Golden vectors for the payment card and IBAN rules. Test key: bytes 0x00..=0x1f.
//!
//! Changing any expected value here is a breaking change: every user's replacements change.

use sagiri_core::engine::Engine;
use sagiri_core::key::Key;
use sagiri_core::pseudo::Pseudonymizer;
use sagiri_core::rules::default_rules;

fn test_engine() -> Engine {
    let mut b = [0u8; 32];
    for (i, x) in b.iter_mut().enumerate() {
        *x = i as u8;
    }
    Engine::new(Pseudonymizer::new(&Key::from_bytes(b)), default_rules())
}

#[test]
fn card_vectors() {
    let e = test_engine();
    let cases: [(&str, &str); 3] = [
        ("4111 1111 1111 1111", "4304 2320 3316 5444"),
        ("378282246310005", "375794859192622"),
        ("6011-1111-1111-1117", "6011-7419-0292-6792"),
    ];
    for (input, want) in cases {
        assert_eq!(e.obfuscate(input), want, "{input}");
    }
}

#[test]
fn iban_vectors() {
    let e = test_engine();
    let cases: [(&str, &str); 3] = [
        ("DE89 3704 0044 0532 0130 00", "DE65 0850 0825 4923 2484 28"),
        ("GB82WEST12345698765432", "GB04CAXG88368763297967"),
        ("fr1420041010050500013m02606", "fr2191471196040247429f48396"),
    ];
    for (input, want) in cases {
        assert_eq!(e.obfuscate(input), want, "{input}");
    }
}
