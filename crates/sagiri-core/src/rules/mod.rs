//! The default rule set. Rules are listed in `Kind` order.
//!
//! Each submodule holds one rule. Some also export the mapping behind the rule, so that
//! other code can replace a value of that kind the same way the rule does.

use crate::rule::Rule;

pub mod card;
pub mod email;
pub mod hostname;
pub mod iban;
pub mod ip;
pub mod phone;
pub mod term;
pub mod url;

/// All built-in rules, in `Kind` order: url, email, ipv4, ipv6, hostname,
/// payment_card, iban, phone. The term rule is not included: it needs the user's terms
/// and is built with [`term::TermRule::new`].
///
/// ```
/// use sagiri_core::engine::Engine;
/// use sagiri_core::rule::Kind;
/// use sagiri_core::rules::{default_rules, term::TermRule};
/// # use sagiri_core::{key::Key, pseudo::Pseudonymizer};
/// # let key = Key::from_bytes(std::array::from_fn(|i| i as u8));
///
/// let kinds: Vec<Kind> = default_rules().iter().map(|r| r.kind()).collect();
/// assert_eq!(kinds, Kind::ALL[1..]);
///
/// // With the user's terms, the term rule goes first, at its place in `Kind` order.
/// let mut rules = default_rules();
/// rules.insert(0, Box::new(TermRule::new(&["Zorblax".to_string()])?));
/// let engine = Engine::new(Pseudonymizer::new(&key), rules);
/// assert!(!engine.obfuscate("Zorblax on 192.0.2.10").contains("Zorblax"));
/// # Ok::<(), Box<dyn std::error::Error>>(())
/// ```
pub fn default_rules() -> Vec<Box<dyn Rule>> {
    vec![
        Box::new(url::UrlRule),
        Box::new(email::EmailRule),
        Box::new(ip::Ipv4Rule),
        Box::new(ip::Ipv6Rule),
        Box::new(hostname::HostnameRule),
        Box::new(card::PaymentCardRule),
        Box::new(iban::IbanRule),
        Box::new(phone::PhoneRule),
    ]
}
