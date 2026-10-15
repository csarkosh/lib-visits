//! `--ignore-ips`: addresses whose beacons are answered and dropped, typically
//! the site owner's own machines. Plain addresses or CIDR prefixes, v4 or v6.
use std::net::IpAddr;

#[derive(Debug, Clone, PartialEq)]
pub struct IpNet {
    addr: IpAddr,
    prefix: u8,
}

impl IpNet {
    /// `203.0.113.9`, `203.0.113.0/24`, `2001:db8::/32`.
    pub fn parse(s: &str) -> Result<IpNet, String> {
        let s = s.trim();
        let (a, p) = match s.split_once('/') { Some((a, p)) => (a, Some(p)), None => (s, None) };
        let addr: IpAddr = a.parse().map_err(|_| format!("not an address: {s}"))?;
        let bits = if addr.is_ipv4() { 32 } else { 128 };
        let prefix = match p {
            None => bits,
            Some(p) => p.parse::<u8>().ok().filter(|n| *n <= bits).ok_or_else(|| format!("bad prefix: {s}"))?,
        };
        Ok(IpNet { addr, prefix })
    }

    pub fn contains(&self, ip: IpAddr) -> bool {
        match (self.addr, ip) {
            (IpAddr::V4(a), IpAddr::V4(b)) => same(u32::from(a).into(), u32::from(b).into(), 32, self.prefix),
            (IpAddr::V6(a), IpAddr::V6(b)) => same(u128::from(a), u128::from(b), 128, self.prefix),
            // A v4 range also covers the v4-mapped v6 form a dual-stack listener may report.
            (IpAddr::V4(a), IpAddr::V6(b)) => b.to_ipv4_mapped().is_some_and(|b| same(u32::from(a).into(), u32::from(b).into(), 32, self.prefix)),
            (IpAddr::V6(_), IpAddr::V4(_)) => false,
        }
    }
}

fn same(a: u128, b: u128, bits: u8, prefix: u8) -> bool {
    if prefix == 0 { return true; }
    let shift = u32::from(bits - prefix);
    (a >> shift) == (b >> shift)
}

/// Every entry parsed, blanks skipped; the first bad one is the error.
pub fn parse_list(items: &[String]) -> Result<Vec<IpNet>, String> {
    items.iter().filter(|s| !s.trim().is_empty()).map(|s| IpNet::parse(s)).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    fn ip(s: &str) -> IpAddr { s.parse().unwrap() }

    #[test]
    fn addresses_and_prefixes() {
        let one = IpNet::parse("203.0.113.9").unwrap();
        assert!(one.contains(ip("203.0.113.9")));
        assert!(!one.contains(ip("203.0.113.10")));
        let net = IpNet::parse("203.0.113.0/24").unwrap();
        assert!(net.contains(ip("203.0.113.255")));
        assert!(!net.contains(ip("203.0.114.0")));
        assert!(net.contains(ip("::ffff:203.0.113.7")), "the v4-mapped form");
        let six = IpNet::parse("2600:4040:9cff:d900::/64").unwrap();
        assert!(six.contains(ip("2600:4040:9cff:d900:5c81:fb9b:25f4:670")));
        assert!(!six.contains(ip("2600:4040:9cff:d901::1")));
        assert!(!six.contains(ip("203.0.113.9")));
        assert!(IpNet::parse("0.0.0.0/0").unwrap().contains(ip("8.8.8.8")));
    }

    #[test]
    fn bad_entries_are_refused() {
        assert!(IpNet::parse("example.com").is_err());
        assert!(IpNet::parse("203.0.113.0/33").is_err());
        assert!(IpNet::parse("2001:db8::/129").is_err());
        assert!(IpNet::parse("203.0.113.0/x").is_err());
        assert_eq!(parse_list(&["203.0.113.9".into(), " ".into(), "2001:db8::/32".into()]).unwrap().len(), 2);
        assert!(parse_list(&["203.0.113.9".into(), "nope".into()]).is_err());
    }
}
