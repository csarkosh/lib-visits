//! The daily update check (the design's 4.6): at most one check a UTC day goes
//! through the collector, and its `rd` says how many days have passed since the
//! last one, which is all an active count and a retention curve need.
//!
//! `Check::next` says whether the update fetch about to happen is the day's
//! check, and `State::checked` records that it was made. The program routes
//! that one fetch through the collector, with `Check::to_query` as its query,
//! and the day's other fetches straight to its update source.

use crate::report::{Check, Envelope};
use crate::state::State;

/// Seconds in a UTC day.
const DAY: u64 = 86_400;

/// The UTC day number of `now`, the days since the epoch.
fn day(now: u64) -> i64 {
    (now / DAY) as i64
}

impl Check {
    /// The check to send with the update fetch the program is about to make,
    /// or `None` when this fetch should go straight to the update source.
    ///
    /// `Some` once a UTC day: `None` when `state.last_check_day` is the day of
    /// `now`. `rd` is the days since that last check, `-1` without one; a clock
    /// set back before it gives `0`, never a negative count. A source build
    /// (`env.ch` is `source`) is always `None`, whatever the state, since it
    /// never touches the collector.
    pub fn next(env: &Envelope, state: &State, now: u64) -> Option<Check> {
        if env.ch.is_source() {
            return None;
        }
        let today = day(now);
        let rd = match state.last_check_day {
            Some(last) if last == today => return None,
            Some(last) => (today - last).max(0),
            None => -1,
        };
        Some(Check { env: env.clone(), rd })
    }
}

impl State {
    /// Records that the day's check was made at `now`. The program calls it
    /// once the request has been made, whatever it answered (a redirect, a
    /// refusal or an error), so a failing collector costs one check a day and
    /// no more. The program calls it through `State::update`, so the state is
    /// loaded just before and saved just after.
    pub fn checked(&mut self, now: u64) {
        self.last_check_day = Some(day(now));
    }
}
