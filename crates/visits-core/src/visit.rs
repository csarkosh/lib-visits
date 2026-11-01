//! The visit: a keyed hash that cannot outlive the day and never stores the
//! address. `day_salt = HMAC(secret, "YYYY-MM-DD")`, `visit = HMAC(day_salt,
//! ip + "\n" + ua)[0..16]`, hex. The visitor (`--visitor`, off by default) is
//! the same without the day: `HMAC(secret, "visitor\n" + ip + "\n" + ua)[0..16]`,
//! so a browser that comes back another day is recognised while its address holds.

use crate::clock::ymd;
use hmac::{Hmac, Mac};
use sha2::Sha256;

type HmacSha256 = Hmac<Sha256>;

fn hmac(key: &[u8], msg: &[u8]) -> [u8; 32] {
    let mut mac = HmacSha256::new_from_slice(key).expect("hmac takes any key length");
    mac.update(msg);
    mac.finalize().into_bytes().into()
}

pub fn visit_hash(secret: &[u8], unix_secs: u64, ip: &str, user_agent: &str) -> String {
    let (y, m, d) = ymd(unix_secs);
    let day_salt = hmac(secret, format!("{y:04}-{m:02}-{d:02}").as_bytes());
    let full = hmac(&day_salt, format!("{ip}\n{user_agent}").as_bytes());
    hex16(&full)
}

/// The visitor: keyed on the secret alone, so it holds across days for as long as the
/// address and the User-Agent do. Written only when the operator asks for it.
pub fn visitor_hash(secret: &[u8], ip: &str, user_agent: &str) -> String {
    hex16(&hmac(secret, format!("visitor\n{ip}\n{user_agent}").as_bytes()))
}

fn hex16(full: &[u8; 32]) -> String {
    full[..8].iter().map(|b| format!("{b:02x}")).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn same_day_same_input_same_visit_and_the_next_day_differs() {
        let a = visit_hash(b"test-secret", 1_792_032_030, "203.0.113.9", "UA");
        let b = visit_hash(b"test-secret", 1_792_032_030 + 3600, "203.0.113.9", "UA");
        let c = visit_hash(b"test-secret", 1_792_032_030 + 86_400, "203.0.113.9", "UA");
        assert_eq!(a, b);
        assert_ne!(a, c);
        assert_eq!(a.len(), 16);
        assert!(a.chars().all(|ch| ch.is_ascii_hexdigit() && !ch.is_ascii_uppercase()));
        assert_ne!(a, visit_hash(b"other-secret", 1_792_032_030, "203.0.113.9", "UA"));
        assert_ne!(a, visit_hash(b"test-secret", 1_792_032_030, "203.0.113.9", "UA2"));
    }

    #[test]
    fn the_visitor_holds_across_days_and_differs_from_the_visit() {
        let a = visitor_hash(b"test-secret", "203.0.113.9", "UA");
        assert_eq!(a.len(), 16);
        assert!(a.chars().all(|ch| ch.is_ascii_hexdigit() && !ch.is_ascii_uppercase()));
        assert_eq!(a, visitor_hash(b"test-secret", "203.0.113.9", "UA"));
        assert_ne!(a, visitor_hash(b"test-secret", "203.0.113.9", "UA2"));
        assert_ne!(a, visitor_hash(b"test-secret", "203.0.113.10", "UA"));
        assert_ne!(a, visitor_hash(b"other-secret", "203.0.113.9", "UA"));
        assert_ne!(a, visit_hash(b"test-secret", 1_792_032_030, "203.0.113.9", "UA"));
    }

    #[test]
    fn the_view_fixture_carries_the_real_value() {
        let f: serde_json::Value = serde_json::from_str(include_str!("../../../spec/fixtures/view.json")).unwrap();
        let expected = visit_hash(b"test-secret", 1_792_032_030, "203.0.113.9", f["facts"]["user_agent"].as_str().unwrap());
        assert_eq!(f["out"]["s"]["visit"], expected, "put this value in spec/fixtures/view.json: {expected}");
    }
}
