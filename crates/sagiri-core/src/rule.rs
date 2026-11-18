//! Rule trait, finding kinds and the context handed to rules.

use std::ops::Range;

use crate::engine::Engine;
use crate::pseudo::Pseudonymizer;

/// Maximum nesting depth of `Context::nested`.
const MAX_DEPTH: u8 = 8;

/// The kind of a sensitive value, in rule order.
#[derive(Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Debug)]
pub enum Kind {
    Term,
    Url,
    Email,
    Ipv4,
    Ipv6,
    Hostname,
    PaymentCard,
    Iban,
    Phone,
}

impl Kind {
    pub const ALL: [Kind; 9] = [
        Kind::Term,
        Kind::Url,
        Kind::Email,
        Kind::Ipv4,
        Kind::Ipv6,
        Kind::Hostname,
        Kind::PaymentCard,
        Kind::Iban,
        Kind::Phone,
    ];

    /// Stable machine-readable identifier.
    pub fn id(self) -> &'static str {
        match self {
            Kind::Term => "term",
            Kind::Url => "url",
            Kind::Email => "email",
            Kind::Ipv4 => "ipv4",
            Kind::Ipv6 => "ipv6",
            Kind::Hostname => "hostname",
            Kind::PaymentCard => "payment_card",
            Kind::Iban => "iban",
            Kind::Phone => "phone",
        }
    }

    /// Human-readable label.
    pub fn label(self) -> &'static str {
        match self {
            Kind::Term => "Term",
            Kind::Url => "URL",
            Kind::Email => "Email",
            Kind::Ipv4 => "IPv4",
            Kind::Ipv6 => "IPv6",
            Kind::Hostname => "Hostname",
            Kind::PaymentCard => "Card",
            Kind::Iban => "IBAN",
            Kind::Phone => "Phone",
        }
    }
}

/// A detector and replacer for one kind of value.
pub trait Rule: Send + Sync {
    fn kind(&self) -> Kind;
    /// Byte ranges in `text` that this rule wants to replace. Already validated.
    fn detect(&self, text: &str) -> Vec<Range<usize>>;
    /// Replacement for one matched string.
    fn replace(&self, original: &str, ctx: &Context<'_>) -> String;
}

/// Access to the engine for rules while replacing.
pub struct Context<'a> {
    pub(crate) engine: &'a Engine,
    pub(crate) depth: u8,
}

impl Context<'_> {
    pub fn pseudo(&self) -> &Pseudonymizer {
        self.engine.pseudo()
    }

    /// Runs the whole engine over a fragment of the matched string. At depth 8 the
    /// fragment is returned unchanged.
    pub fn nested(&self, fragment: &str) -> String {
        if self.depth >= MAX_DEPTH {
            return fragment.to_string();
        }
        self.engine.obfuscate_at(fragment, self.depth + 1)
    }

    /// True if the user's allowlist covers `value`.
    pub fn is_allowed(&self, value: &str) -> bool {
        self.engine.is_allowed(value)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    #[test]
    fn kind_all_is_distinct_and_ids_unique() {
        let kinds: HashSet<Kind> = Kind::ALL.iter().copied().collect();
        assert_eq!(kinds.len(), 9);
        let ids: HashSet<&str> = Kind::ALL.iter().map(|k| k.id()).collect();
        assert_eq!(ids.len(), 9);
        let labels: HashSet<&str> = Kind::ALL.iter().map(|k| k.label()).collect();
        assert_eq!(labels.len(), 9);
        assert!(Kind::ALL.windows(2).all(|w| w[0] < w[1]));
    }
}
