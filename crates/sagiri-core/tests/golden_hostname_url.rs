//! Golden vectors for the hostname and URL rules. Test key: bytes 0x00..=0x1f.
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
fn hostname_vectors() {
    let e = test_engine();
    let cases: [(&str, &str); 5] = [
        ("acme.com", "piti.com"),
        ("git.acme.com", "ita.piti.com"),
        ("www.acme.co.uk", "por.uzev.co.uk"),
        ("db-prod-01.corp.internal", "ek-roga-34.okal.internal"),
        ("ACME.github.io", "VINA.github.io"),
    ];
    for (input, want) in cases {
        assert_eq!(e.obfuscate(input), want, "{input}");
    }
}

#[test]
fn url_vectors() {
    let e = test_engine();
    let cases: [(&str, &str); 3] = [
        (
            "https://git.acme.com/team/repo",
            "https://ita.piti.com/team/repo",
        ),
        (
            "postgres://admin:s3cret@db01.corp.internal:5432/app?sslmode=require",
            "postgres://petin:o0ateb@ud65.okal.internal:5432/app?sslmode=adadefe",
        ),
        (
            "http://10.0.0.5:8080/nodes/10.0.0.6?peer=10.0.0.7#top",
            "http://10.108.232.39:8080/nodes/10.253.155.228?peer=10.167.118.110#top",
        ),
    ];
    for (input, want) in cases {
        assert_eq!(e.obfuscate(input), want, "{input}");
    }
}

#[test]
fn url_host_matches_bare_values() {
    let e = test_engine();
    let host = e.obfuscate("git.acme.com");
    let ip = e.obfuscate("10.0.0.5");
    assert_eq!(
        e.obfuscate("git.acme.com https://git.acme.com/x 10.0.0.5 http://10.0.0.5:8080/x"),
        format!("{host} https://{host}/x {ip} http://{ip}:8080/x")
    );
}
