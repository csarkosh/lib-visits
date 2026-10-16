//! Fresh identifiers for a session's `sid` and a crash's `inc`.

use std::collections::hash_map::RandomState;
use std::hash::{BuildHasher, Hasher};
use std::sync::atomic::{AtomicU64, Ordering};

/// A fresh identifier: 32 lowercase hex characters of randomness, from two
/// hashers of the standard library's `RandomState`, whose keys are seeded from
/// the operating system's randomness. It is the only source a program should
/// use for a session's `sid` and a crash's `inc`: an identifier made from
/// anything about the person or the machine would make reports linkable.
pub fn new_id() -> String {
    // A count of the calls in this process, so two calls never hash the same input.
    static CALLS: AtomicU64 = AtomicU64::new(0);
    let n = CALLS.fetch_add(1, Ordering::Relaxed);
    let half = || {
        let mut h = RandomState::new().build_hasher();
        h.write_u64(n);
        h.finish()
    };
    format!("{:016x}{:016x}", half(), half())
}
