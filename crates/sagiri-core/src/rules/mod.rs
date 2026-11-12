//! The default rule set. Rules are listed in `Kind` order.

use crate::rule::Rule;

pub mod ip;

/// All built-in rules, in `Kind` order: term, url, email, ipv4, ipv6, hostname,
/// payment_card, iban, phone.
pub fn default_rules() -> Vec<Box<dyn Rule>> {
    vec![Box::new(ip::Ipv4Rule), Box::new(ip::Ipv6Rule)]
}
