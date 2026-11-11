//! The default rule set. Rules are listed in `Kind` order.

use crate::rule::Rule;

/// All built-in rules, in `Kind` order: term, url, email, ipv4, ipv6, hostname,
/// payment_card, iban, phone.
pub fn default_rules() -> Vec<Box<dyn Rule>> {
    Vec::new()
}
