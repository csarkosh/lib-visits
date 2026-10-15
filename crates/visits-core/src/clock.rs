//! UTC dates without a date crate: days since the epoch to a civil date, by
//! Howard Hinnant's algorithm.

pub fn ymd(unix_secs: u64) -> (i64, u32, u32) {
    let days = (unix_secs / 86_400) as i64;
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    (if m <= 2 { y + 1 } else { y }, m, d)
}

pub fn rfc3339(unix_secs: u64) -> String {
    let (y, m, d) = ymd(unix_secs);
    let s = unix_secs % 86_400;
    format!("{y:04}-{m:02}-{d:02}T{:02}:{:02}:{:02}Z", s / 3600, (s % 3600) / 60, s % 60)
}

/// The same instant with milliseconds: `2026-10-15T02:40:30.250Z`.
pub fn rfc3339_ms(unix_ms: u64) -> String {
    let whole = rfc3339(unix_ms / 1000);
    format!("{}.{:03}Z", &whole[..whole.len() - 1], unix_ms % 1000)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn with_milliseconds() {
        assert_eq!(rfc3339_ms(1_792_032_030_250), "2026-10-15T02:40:30.250Z");
        assert_eq!(rfc3339_ms(0), "1970-01-01T00:00:00.000Z");
    }
    #[test]
    fn known_dates() {
        assert_eq!(ymd(0), (1970, 1, 1));
        assert_eq!(ymd(951_782_400), (2000, 2, 29));
        assert_eq!(rfc3339(1_792_032_030), "2026-10-15T02:40:30Z");
        assert_eq!(rfc3339(1_792_032_000), "2026-10-15T02:40:00Z");
    }
}
