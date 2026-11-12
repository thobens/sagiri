//! Golden vectors for the IP rules. Test key: bytes 0x00..=0x1f.
//!
//! Changing any expected value here is a breaking change: every user's replacements change.

use sagiri_core::engine::Engine;
use sagiri_core::key::Key;
use sagiri_core::pseudo::Pseudonymizer;
use sagiri_core::rules::default_rules;

fn key_from(fill: impl Fn(usize) -> u8) -> Key {
    let mut b = [0u8; 32];
    for (i, x) in b.iter_mut().enumerate() {
        *x = fill(i);
    }
    Key::from_bytes(b)
}

fn engine(key: &Key) -> Engine {
    Engine::new(Pseudonymizer::new(key), default_rules())
}

fn test_engine() -> Engine {
    engine(&key_from(|i| i as u8))
}

#[test]
fn ip_vectors() {
    let e = test_engine();
    let cases: [(&str, &str); 10] = [
        ("10.1.2.3", "10.154.112.225"),
        ("172.16.5.6", "172.30.234.48"),
        ("192.168.1.10", "192.168.26.146"),
        ("169.254.9.9", "169.254.125.6"),
        ("100.64.0.1", "100.66.11.195"),
        ("172.55.0.1", "125.91.180.30"),
        ("fe80::1234:5678", "fe80::ce20:4542:cb7c:4f4"),
        ("fd12:3456::1", "fdbd:1b2b:3c52:45b7:7e86:bf55:d05a:644c"),
        ("2001:db8::1", "3f0c:37ea:8c11:a1a7:9cb1:1d09:d2b3:3305"),
        ("64:ff9b::1", "64:9248:c66e:3859:9ba1:edff:51c1:e9a4"),
    ];
    for (input, want) in cases {
        assert_eq!(e.obfuscate(input), want, "{input}");
    }
    assert_eq!(e.obfuscate("::ffff:192.168.1.10"), "::ffff:192.168.26.146");
}

#[test]
fn reference_example() {
    let text = "src 172.55.0.1 dst 172.55.0.1";
    let e = test_engine();
    let out = e.obfuscate(text);
    let findings = e.analyze(text);
    assert_eq!(findings.len(), 2);
    assert_eq!(findings[0].replacement, findings[1].replacement);
    let repl: std::net::Ipv4Addr = findings[0].replacement.parse().unwrap();
    assert_ne!(repl.to_string(), "172.55.0.1");
    // Public: not private, loopback, multicast or similar.
    assert!(!repl.is_private() && !repl.is_loopback() && !repl.is_link_local());
    assert!(repl.octets()[0] != 0 && repl.octets()[0] < 224);
    let o = repl.octets();
    assert!(!(o[0] == 100 && (64..128).contains(&o[1])));
    assert_eq!(out, format!("src {repl} dst {repl}"));

    // Same key, new engine: same output.
    assert_eq!(test_engine().obfuscate(text), out);
    // Different key: different output.
    let other = engine(&key_from(|i| 0xff - i as u8));
    assert_ne!(other.obfuscate(text), out);
}
