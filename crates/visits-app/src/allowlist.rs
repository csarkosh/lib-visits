//! The program's own schema of events: which names it records, which `d` keys
//! each may carry and, per key, the values it may take; and which surfaces its
//! focus time may be counted against. The file is parsed once at start-up;
//! anything the wire would refuse is refused there, so an event the allowlist
//! admits is an event the collector accepts.
//!
//! The file's shape: one `[events.<name>]` table per event, one key per
//! permitted `d` key, and per key either the list of values it may take,
//! `"int"` or `"bool"`; and one `[focus]` table whose `surfaces` lists, in at
//! most 16 names, the surfaces `Journal::focus` may count. A file without a
//! `[focus]` table allows no surface.
//!
//! ```toml
//! [events.pane_open]
//! kind = ["terminal", "viewer", "browser"]
//! how = ["palette", "key", "click"]
//! [events.window_open]
//! [focus]
//! surfaces = ["terminal", "viewer", "browser"]
//! ```

use crate::report::{DValue, Event};
use std::collections::{BTreeMap, BTreeSet};
use toml::{Table, Value};
use visits_core::app::{is_name, is_value, MAX_FOCUS, MAX_KEYS, MAX_NAME, MAX_VALUE};

/// Why `Allowlist::check` turned an event away: the event's name is not listed;
/// the event carries a key its entry does not list (event, key); or a key's
/// value is not one its entry permits (event, key).
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum Rejected {
    Name(String),
    Key(String, String),
    Value(String, String),
}

/// What a `d` key admits.
#[derive(Clone, PartialEq, Eq, Debug)]
enum Allowed {
    /// One of these strings.
    OneOf(BTreeSet<String>),
    /// Any integer.
    Int,
    /// `true` or `false`.
    Bool,
}

impl Allowed {
    fn admits(&self, v: &DValue) -> bool {
        match (self, v) {
            (Allowed::OneOf(set), DValue::Str(s)) => set.contains(s),
            (Allowed::Int, DValue::Int(_)) => true,
            (Allowed::Bool, DValue::Bool(_)) => true,
            _ => false,
        }
    }
}

/// The parsed allowlist: per event name, its `d` keys and what each admits,
/// and the surfaces focus time may be counted against.
#[derive(Clone, Default, PartialEq, Eq, Debug)]
pub struct Allowlist {
    events: BTreeMap<String, BTreeMap<String, Allowed>>,
    surfaces: BTreeSet<String>,
}

impl Allowlist {
    /// Parses the allowlist file (its shape is in the crate's documentation).
    /// The error names the event and key at fault, for the program's author.
    pub fn from_toml(text: &str) -> Result<Allowlist, String> {
        let table: Table = text.parse().map_err(|e| format!("allowlist: {e}"))?;
        if let Some(key) = table.keys().find(|k| *k != "events" && *k != "focus") {
            return Err(format!("allowlist: unknown table {key:?}, expected only [events.<name>] and [focus]"));
        }
        let surfaces = match table.get("focus") {
            Some(focus) => parse_focus(focus)?,
            None => BTreeSet::new(),
        };
        let mut events = BTreeMap::new();
        let Some(listed) = table.get("events") else { return Ok(Allowlist { events, surfaces }) };
        let listed = listed.as_table().ok_or("allowlist: events is not a table")?;
        for (name, keys) in listed {
            if !is_name(name) {
                return Err(format!("allowlist: bad event name {name:?}, expected [a-z][a-z0-9_] and at most {MAX_NAME}"));
            }
            let keys = keys.as_table().ok_or_else(|| format!("allowlist: event {name} is not a table"))?;
            if keys.len() > MAX_KEYS {
                return Err(format!("allowlist: event {name} has more than {MAX_KEYS} keys"));
            }
            let mut allowed = BTreeMap::new();
            for (key, value) in keys {
                if !is_name(key) {
                    return Err(format!("allowlist: event {name}: bad key {key:?}, expected [a-z][a-z0-9_] and at most {MAX_NAME}"));
                }
                allowed.insert(key.clone(), parse_allowed(value).map_err(|e| format!("allowlist: event {name}, key {key}: {e}"))?);
            }
            events.insert(name.clone(), allowed);
        }
        Ok(Allowlist { events, surfaces })
    }

    /// Whether the program may record this event: its name is listed, each of
    /// its keys is listed under that name, and each value is one the key admits.
    /// A listed key the event leaves out is fine.
    pub fn check(&self, ev: &Event) -> Result<(), Rejected> {
        let keys = self.events.get(&ev.n).ok_or_else(|| Rejected::Name(ev.n.clone()))?;
        for (key, value) in &ev.d {
            let allowed = keys.get(key).ok_or_else(|| Rejected::Key(ev.n.clone(), key.clone()))?;
            if !allowed.admits(value) {
                return Err(Rejected::Value(ev.n.clone(), key.clone()));
            }
        }
        Ok(())
    }

    /// Whether `Journal::focus` may count time against this surface: it is
    /// listed in the file's `[focus]` table.
    pub fn allows_surface(&self, surface: &str) -> bool {
        self.surfaces.contains(surface)
    }
}

/// The `[focus]` table: `surfaces`, a list of at most 16 names, and nothing else.
fn parse_focus(focus: &Value) -> Result<BTreeSet<String>, String> {
    let focus = focus.as_table().ok_or("allowlist: focus is not a table")?;
    if let Some(key) = focus.keys().find(|k| *k != "surfaces") {
        return Err(format!("allowlist: focus: unknown key {key:?}, expected only surfaces"));
    }
    let Some(listed) = focus.get("surfaces") else { return Ok(BTreeSet::new()) };
    let listed = listed.as_array().ok_or("allowlist: focus: surfaces is not a list")?;
    let mut surfaces = BTreeSet::new();
    for item in listed {
        let s = item.as_str().ok_or_else(|| format!("allowlist: focus: {item} is not a string"))?;
        if !is_name(s) {
            return Err(format!("allowlist: focus: bad surface {s:?}, expected [a-z][a-z0-9_] and at most {MAX_NAME}"));
        }
        surfaces.insert(s.to_string());
    }
    if surfaces.len() > MAX_FOCUS {
        return Err(format!("allowlist: focus: more than {MAX_FOCUS} surfaces"));
    }
    Ok(surfaces)
}

/// A key's entry: a list of strings, or `"int"` or `"bool"`.
fn parse_allowed(value: &Value) -> Result<Allowed, String> {
    match value {
        Value::String(s) if s == "int" => Ok(Allowed::Int),
        Value::String(s) if s == "bool" => Ok(Allowed::Bool),
        Value::String(s) => Err(format!("{s:?} is not \"int\" or \"bool\"")),
        Value::Array(items) => {
            let mut set = BTreeSet::new();
            for item in items {
                let s = item.as_str().ok_or_else(|| format!("{item} is not a string"))?;
                if !is_value(s) {
                    return Err(format!("bad value {s:?}, expected [A-Za-z0-9_.:/-] and at most {MAX_VALUE}"));
                }
                set.insert(s.to_string());
            }
            Ok(Allowed::OneOf(set))
        }
        other => Err(format!("{other} is not a list of values, \"int\" or \"bool\"")),
    }
}
