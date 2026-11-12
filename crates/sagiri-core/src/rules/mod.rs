//! The default rule set. Rules are listed in `Kind` order.

use crate::rule::Rule;

pub mod card;
pub mod email;
pub mod hostname;
pub mod iban;
pub mod ip;
pub mod phone;
pub mod url;

/// All built-in rules, in `Kind` order: term, url, email, ipv4, ipv6, hostname,
/// payment_card, iban, phone.
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
