//! Golden vectors for the phone rule. Test key: bytes 0x00..=0x1f.
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
fn phone_vectors() {
    let e = test_engine();
    let cases: [(&str, &str); 4] = [
        ("+41 79 123 45 67", "+41 75 695 56 08"),
        ("+41 (0)79 123 45 67", "+41 (0)75 695 56 08"),
        ("+1 (415) 555-0132", "+1 (714) 402-2749"),
        ("+44 20 7946 0958", "+44 54 8368 0537"),
    ];
    for (input, want) in cases {
        assert_eq!(e.obfuscate(input), want, "{input}");
    }
}
