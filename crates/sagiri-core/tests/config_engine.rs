//! The allowlist, terms and rule toggles in a running engine.

use sagiri_core::config::{Allowlist, Config};
use sagiri_core::engine::Engine;
use sagiri_core::key::Key;
use sagiri_core::pseudo::Pseudonymizer;
use sagiri_core::rule::Rule;
use sagiri_core::rules::default_rules;
use sagiri_core::rules::term::TermRule;

/// Builds an engine the way the binary does.
fn engine(config: &Config) -> Engine {
    let mut b = [0u8; 32];
    for (i, x) in b.iter_mut().enumerate() {
        *x = i as u8;
    }
    let mut rules: Vec<Box<dyn Rule>> = default_rules()
        .into_iter()
        .filter(|r| config.rules.enabled(r.kind()))
        .collect();
    if config.rules.term && !config.terms.is_empty() {
        rules.insert(0, Box::new(TermRule::new(&config.terms).unwrap()));
    }
    Engine::new(Pseudonymizer::new(&Key::from_bytes(b)), rules)
        .with_allowlist(Allowlist::new(&config.allow).unwrap())
}

fn strings(v: &[&str]) -> Vec<String> {
    v.iter().map(|s| s.to_string()).collect()
}

#[test]
fn allowlist_keeps_values() {
    let e = engine(&Config {
        allow: strings(&["10.20.0.0/16", "*.github.com", "intranet.acme.com"]),
        ..Config::default()
    });
    for kept in [
        "10.20.3.4",
        "api.github.com",
        "https://api.github.com/x",
        "http://10.20.3.4:8080/",
    ] {
        assert!(e.analyze(kept).is_empty(), "{kept}");
        assert_eq!(e.obfuscate(kept), kept);
    }
    let out = e.obfuscate("jane@intranet.acme.com");
    assert!(out.ends_with("@intranet.acme.com"), "{out}");
    assert!(!out.starts_with("jane@"), "{out}");
    assert_ne!(e.obfuscate("10.21.0.1"), "10.21.0.1");
    assert_ne!(e.obfuscate("acme.com"), "acme.com");
}

#[test]
fn allowing_a_domain_does_not_allow_its_subdomains() {
    let e = engine(&Config {
        allow: strings(&["github.com"]),
        ..Config::default()
    });
    assert_eq!(
        e.obfuscate("https://github.com/org/repo"),
        "https://github.com/org/repo"
    );
    assert_ne!(e.obfuscate("api.github.com"), "api.github.com");
}

#[test]
fn toggles_switch_rules_off() {
    let mut config = Config::default();
    config.rules.ipv4 = false;
    let e = engine(&config);
    assert_eq!(e.obfuscate("198.51.100.7"), "198.51.100.7");
    assert_ne!(e.obfuscate("git.acme.com"), "git.acme.com");

    let mut config = Config {
        terms: strings(&["Acme Corp"]),
        ..Config::default()
    };
    assert_ne!(engine(&config).obfuscate("Acme Corp"), "Acme Corp");
    config.rules.term = false;
    assert_eq!(engine(&config).obfuscate("Acme Corp"), "Acme Corp");
}

#[test]
fn terms_work_next_to_other_rules() {
    let e = engine(&Config {
        terms: strings(&["Project Falcon"]),
        ..Config::default()
    });
    let out = e.obfuscate("Project Falcon runs on 198.51.100.7");
    assert!(
        !out.contains("Falcon") && !out.contains("198.51.100.7"),
        "{out}"
    );
}
