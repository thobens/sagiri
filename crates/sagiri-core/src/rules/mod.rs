//! The default rule set. Rules are listed in `Kind` order.

use crate::rule::Rule;

pub mod hostname;
pub mod ip;
pub mod url;

/// All built-in rules, in `Kind` order: term, url, email, ipv4, ipv6, hostname,
/// payment_card, iban, phone.
pub fn default_rules() -> Vec<Box<dyn Rule>> {
    vec![
        Box::new(url::UrlRule),
        Box::new(ip::Ipv4Rule),
        Box::new(ip::Ipv6Rule),
        Box::new(hostname::HostnameRule),
    ]
}
