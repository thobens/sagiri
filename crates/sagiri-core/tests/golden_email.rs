//! Golden vectors for the email rule. Test key: bytes 0x00..=0x1f.
//!
//! Changing any expected value here is a breaking change: every user's replacements change.

use sagiri_core::engine::Engine;
use sagiri_core::key::Key;
use sagiri_core::pseudo::Pseudonymizer;
use sagiri_core::rule::Kind;
use sagiri_core::rules::default_rules;

fn test_engine() -> Engine {
    let mut b = [0u8; 32];
    for (i, x) in b.iter_mut().enumerate() {
        *x = i as u8;
    }
    Engine::new(Pseudonymizer::new(&Key::from_bytes(b)), default_rules())
}

#[test]
fn email_vectors() {
    let e = test_engine();
    let cases: [(&str, &str); 5] = [
        ("john.doe@acme.com", "fizo.eta@piti.com"),
        ("John.Doe@ACME.com", "Fizo.Eta@PITI.com"),
        (
            "first_last+tag-1@mail.acme.co.uk",
            "eniko_vatu+sep-1@rafo.uzev.co.uk",
        ),
        ("jane@example.com", "pozu@example.com"),
        ("ops-team@db01.corp.internal", "zet-osos@ud65.okal.internal"),
    ];
    for (input, want) in cases {
        assert_eq!(e.obfuscate(input), want, "{input}");
    }
}

#[test]
fn email_domain_matches_bare_hostname() {
    let e = test_engine();
    let bare = e.obfuscate("acme.com");
    let out = e.obfuscate("john.doe@acme.com");
    assert_eq!(out.split_once('@').unwrap().1, bare);
    assert_eq!(e.obfuscate("x john.doe@acme.com y"), format!("x {out} y"));
}

#[test]
fn email_in_url_query_matches_bare_address() {
    let e = test_engine();
    let bare = e.obfuscate("john@acme.com");
    let url = e.obfuscate("https://acme.com/u?mail=john@acme.com");
    assert!(url.ends_with(&format!("?mail={bare}")), "{url}");
}

#[test]
fn userinfo_is_not_an_email() {
    let e = test_engine();
    let findings = e.analyze("https://john:pw@acme.com/x");
    assert_eq!(findings.len(), 1);
    assert_eq!(findings[0].kind, Kind::Url);
}
