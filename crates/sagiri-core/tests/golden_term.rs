//! Golden vectors for the term rule. Test key: bytes 0x00..=0x1f.
//!
//! Changing any expected value here is a breaking change: every user's replacements change.

use sagiri_core::engine::Engine;
use sagiri_core::key::Key;
use sagiri_core::pseudo::Pseudonymizer;
use sagiri_core::rule::Rule;
use sagiri_core::rules::default_rules;
use sagiri_core::rules::term::TermRule;

fn test_engine() -> Engine {
    let mut b = [0u8; 32];
    for (i, x) in b.iter_mut().enumerate() {
        *x = i as u8;
    }
    let terms = ["Acme Corp", "Project Falcon", "Zorblax"].map(String::from);
    let mut rules: Vec<Box<dyn Rule>> = vec![Box::new(TermRule::new(&terms).unwrap())];
    rules.extend(default_rules());
    Engine::new(Pseudonymizer::new(&Key::from_bytes(b)), rules)
}

#[test]
fn term_vectors() {
    let e = test_engine();
    let cases: [(&str, &str); 4] = [
        ("Acme Corp", "Sifi Voka"),
        ("ACME CORP", "SIFI VOKA"),
        ("Project Falcon", "Olipumi Mesura"),
        ("zorblax", "kazanod"),
    ];
    for (input, want) in cases {
        assert_eq!(e.obfuscate(input), want, "{input}");
    }
}
