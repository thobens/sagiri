//! Rule trait, finding kinds and the context handed to rules.
//!
//! A [`Rule`] finds one [`Kind`] of value and derives its replacement. The
//! [`Engine`] runs the rules and hands each a [`Context`].

use std::ops::Range;

use crate::engine::Engine;
use crate::pseudo::Pseudonymizer;

/// Maximum nesting depth of `Context::nested`.
const MAX_DEPTH: u8 = 8;

/// The kind of a sensitive value, in rule order.
///
/// The order of the variants is the order of
/// [`default_rules`](crate::rules::default_rules), and `Ord` follows it.
#[derive(Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Debug)]
pub enum Kind {
    /// A literal term from the user's config.
    Term,
    /// A URL with a `scheme://` prefix.
    Url,
    /// An email address.
    Email,
    /// An IPv4 address in dotted decimal.
    Ipv4,
    /// An IPv6 address.
    Ipv6,
    /// A hostname with a known suffix.
    Hostname,
    /// A payment card number.
    PaymentCard,
    /// An international bank account number.
    Iban,
    /// A phone number in international form.
    Phone,
}

impl Kind {
    /// Every kind, in rule order.
    ///
    /// ```
    /// use sagiri_core::rule::Kind;
    ///
    /// assert_eq!(Kind::ALL.len(), 9);
    /// assert_eq!(Kind::ALL[0], Kind::Term);
    /// assert!(Kind::ALL.windows(2).all(|w| w[0] < w[1]));
    /// ```
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
    ///
    /// The identifier is the key of the kind in the `[rules]` table of the config file and
    /// does not change between versions.
    ///
    /// ```
    /// use sagiri_core::rule::Kind;
    ///
    /// assert_eq!(Kind::PaymentCard.id(), "payment_card");
    /// assert_eq!(Kind::Ipv6.id(), "ipv6");
    /// ```
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

    /// Human-readable label, short enough for a column in a terminal.
    ///
    /// Labels are for display only and may change; use [`id`](Self::id) to identify a
    /// kind.
    ///
    /// ```
    /// use sagiri_core::rule::Kind;
    ///
    /// assert_eq!(Kind::Ipv4.label(), "IPv4");
    /// assert_eq!(Kind::PaymentCard.label(), "Card");
    /// ```
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
///
/// The [`Engine`] calls [`detect`](Self::detect) on the whole text,
/// resolves overlaps between all rules, and calls [`replace`](Self::replace) for every
/// range it keeps. A rule decides what a value is and what replaces it; the engine
/// decides which match wins.
///
/// A replacement must be a pure function of the key and the original: derive every
/// varying part from [`Context::pseudo`], seeded with the canonical form of the
/// original, so that two spellings of one value get one replacement. Never use another
/// source of randomness.
///
/// # Example
///
/// A rule for `name=value` pairs. It keeps allowed values, lets the other rules replace
/// what they recognise inside the value, and replaces the rest with pseudo-text.
///
/// ```
/// use std::ops::Range;
/// use sagiri_core::config::Allowlist;
/// use sagiri_core::engine::Engine;
/// use sagiri_core::rule::{Context, Kind, Rule};
/// use sagiri_core::rules::ip::Ipv4Rule;
/// # use sagiri_core::{key::Key, pseudo::Pseudonymizer};
/// # let pseudo = Pseudonymizer::new(&Key::from_bytes(std::array::from_fn(|i| i as u8)));
///
/// struct Assignment;
///
/// impl Rule for Assignment {
///     fn kind(&self) -> Kind {
///         Kind::Term
///     }
///
///     fn detect(&self, text: &str) -> Vec<Range<usize>> {
///         let mut out = Vec::new();
///         let mut start = 0;
///         for word in text.split(' ') {
///             if word.contains('=') {
///                 out.push(start..start + word.len());
///             }
///             start += word.len() + 1;
///         }
///         out
///     }
///
///     fn replace(&self, original: &str, ctx: &Context<'_>) -> String {
///         let Some((name, value)) = original.split_once('=') else {
///             return original.to_string();
///         };
///         if ctx.is_allowed(value) {
///             return original.to_string();
///         }
///         let nested = ctx.nested(value);
///         let value = if nested != value {
///             nested
///         } else {
///             ctx.pseudo().mimic("assignment", value.as_bytes(), value)
///         };
///         format!("{name}={value}")
///     }
/// }
///
/// let allow = Allowlist::new(&["198.51.100.1".to_string()])?;
/// let engine = Engine::new(pseudo, vec![Box::new(Assignment), Box::new(Ipv4Rule)])
///     .with_allowlist(allow);
///
/// let out = engine.obfuscate("user=jane addr=192.0.2.10 gw=198.51.100.1");
/// let parts: Vec<&str> = out.split(' ').collect();
/// assert!(parts[0].starts_with("user=") && parts[0] != "user=jane");
/// assert!(parts[1].starts_with("addr=") && parts[1] != "addr=192.0.2.10");
/// assert_eq!(parts[2], "gw=198.51.100.1");
/// # Ok::<(), Box<dyn std::error::Error>>(())
/// ```
pub trait Rule: Send + Sync {
    /// The kind of value this rule finds. Reported in every [`Finding`] of the rule.
    ///
    /// [`Finding`]: crate::engine::Finding
    fn kind(&self) -> Kind;
    /// Byte ranges in `text` that this rule wants to replace. Already validated.
    ///
    /// Report only values the rule is sure about: a false positive mangles the user's
    /// text. Ranges may overlap and come in any order. The engine ignores a range that is
    /// empty, out of bounds, not on character boundaries or contains a line break.
    fn detect(&self, text: &str) -> Vec<Range<usize>>;
    /// Replacement for one matched string.
    ///
    /// `original` is the text of one range from [`detect`](Self::detect). The same
    /// `original` and key must always give the same result. A result equal to `original`
    /// keeps the value; a result with a line break is discarded and the value is kept.
    fn replace(&self, original: &str, ctx: &Context<'_>) -> String;
}

/// Access to the engine for rules while replacing.
///
/// Handed to [`Rule::replace`]. See the example of [`Rule`] for all three methods.
pub struct Context<'a> {
    pub(crate) engine: &'a Engine,
    pub(crate) depth: u8,
}

impl Context<'_> {
    /// The pseudonymizer of the engine, the only source of derived values for a rule.
    pub fn pseudo(&self) -> &Pseudonymizer {
        self.engine.pseudo()
    }

    /// Runs the whole engine over a fragment of the matched string. At depth 8 the
    /// fragment is returned unchanged.
    ///
    /// This is how composite values delegate: the host of a URL gets the same
    /// replacement as the same host standing alone. The allowlist applies inside the
    /// fragment as well.
    pub fn nested(&self, fragment: &str) -> String {
        if self.depth >= MAX_DEPTH {
            return fragment.to_string();
        }
        self.engine.obfuscate_at(fragment, self.depth + 1)
    }

    /// True if the user's allowlist covers `value`.
    ///
    /// The engine already keeps a whole match that the allowlist covers. A rule asks for
    /// the parts of a match, the domain of an email address for example. See
    /// [`Allowlist::matches`](crate::config::Allowlist::matches) for what counts as
    /// covered.
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
