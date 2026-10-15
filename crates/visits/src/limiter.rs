//! A token bucket an address. Bounded: past 4096 addresses the idle ones are
//! dropped, and if none are idle the newcomer is served untracked rather than
//! the process growing without bound.
use std::collections::HashMap;
use std::time::Instant;

struct Bucket { tokens: f64, last: Instant }

pub struct Limiter {
    burst: f64,
    per_second: f64,
    buckets: HashMap<String, Bucket>,
}

impl Limiter {
    pub fn new(burst: f64, per_second: f64) -> Self {
        Limiter { burst, per_second, buckets: HashMap::new() }
    }

    pub fn allow(&mut self, key: &str) -> bool {
        let now = Instant::now();
        if self.buckets.len() >= 4096 && !self.buckets.contains_key(key) {
            self.buckets.retain(|_, b| now.duration_since(b.last).as_secs() < 120);
            if self.buckets.len() >= 4096 { return true; }
        }
        let b = self.buckets.entry(key.to_string()).or_insert(Bucket { tokens: self.burst, last: now });
        let elapsed = now.duration_since(b.last).as_secs_f64();
        b.tokens = (b.tokens + elapsed * self.per_second).min(self.burst);
        b.last = now;
        if b.tokens < 1.0 { return false; }
        b.tokens -= 1.0;
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn burst_then_refuse() {
        let mut l = Limiter::new(2.0, 0.0);
        assert!(l.allow("a"));
        assert!(l.allow("a"));
        assert!(!l.allow("a"));
        assert!(l.allow("b"));
    }
}
