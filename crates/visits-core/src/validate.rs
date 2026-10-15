//! The wire spec's section 2 as code: which keys exist, their types and caps.

use crate::event::Event;
use serde_json::{Map, Value};
use std::collections::BTreeMap;
use std::fmt;

pub const MAX_EVENTS: usize = 50;

#[derive(Debug, PartialEq)]
pub enum Invalid {
    Json,
    TooMany,
    Version,
    Missing(&'static str),
    Name,
    Type(String),
    Long(String),
    Keys(String),
}

impl fmt::Display for Invalid {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Invalid::Json => write!(f, "malformed json"),
            Invalid::TooMany => write!(f, "too many events"),
            Invalid::Version => write!(f, "unknown version"),
            Invalid::Missing(k) => write!(f, "missing {k}"),
            Invalid::Name => write!(f, "bad name"),
            Invalid::Type(k) => write!(f, "bad type: {k}"),
            Invalid::Long(k) => write!(f, "too long: {k}"),
            Invalid::Keys(k) => write!(f, "too many keys: {k}"),
        }
    }
}

impl std::error::Error for Invalid {}

#[derive(Clone, Copy)]
enum Kind {
    Str(usize),
    Int,
    Num,
    Bool,
    Payload,
}

/// Every known key beyond the four required ones, with its type and cap.
const KEYS: &[(&str, Kind)] = &[
    ("t", Kind::Str(200)), ("r", Kind::Str(2048)),
    ("us", Kind::Str(100)), ("um", Kind::Str(100)), ("uc", Kind::Str(100)), ("ut", Kind::Str(100)), ("uo", Kind::Str(100)), ("cr", Kind::Str(100)),
    ("nt", Kind::Str(16)), ("th", Kind::Str(8)), ("tz", Kind::Str(64)), ("lg", Kind::Str(32)), ("ev", Kind::Str(64)), ("u", Kind::Str(2048)),
    ("sw", Kind::Int), ("sh", Kind::Int), ("vw", Kind::Int), ("vh", Kind::Int), ("b", Kind::Int), ("e", Kind::Int), ("sd", Kind::Int),
    ("dpr", Kind::Num),
    ("rm", Kind::Bool), ("tp", Kind::Bool), ("re", Kind::Bool),
    ("d", Kind::Payload),
];

fn kind_of(key: &str) -> Option<Kind> {
    KEYS.iter().find(|(k, _)| *k == key).map(|(_, kind)| *kind)
}

fn good_name(n: &str) -> bool {
    let mut chars = n.chars();
    match chars.next() {
        Some(c) if c.is_ascii_lowercase() => {}
        _ => return false,
    }
    n.len() <= 32 && chars.all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_' || c == '-')
}

fn check_value(key: &str, kind: Kind, v: &Value) -> Result<(), Invalid> {
    let ok = match kind {
        Kind::Str(max) => match v.as_str() {
            Some(s) => { if s.chars().count() > max { return Err(Invalid::Long(key.into())); } true }
            None => false,
        },
        Kind::Int => v.is_i64() || v.is_u64(),
        Kind::Num => v.is_number(),
        Kind::Bool => v.is_boolean(),
        Kind::Payload => match v.as_object() {
            Some(m) => {
                if m.len() > 10 { return Err(Invalid::Keys(key.into())); }
                for (k, inner) in m {
                    let name = format!("{key}.{k}");
                    match inner {
                        Value::String(s) if s.chars().count() > 200 => return Err(Invalid::Long(name)),
                        Value::String(_) | Value::Number(_) | Value::Bool(_) => {}
                        _ => return Err(Invalid::Type(name)),
                    }
                }
                true
            }
            None => false,
        },
    };
    if ok { Ok(()) } else { Err(Invalid::Type(key.into())) }
}

fn required_str(m: &Map<String, Value>, key: &'static str, max: usize) -> Result<String, Invalid> {
    match m.get(key) {
        None => Err(Invalid::Missing(key)),
        Some(Value::String(s)) => {
            if s.is_empty() { return Err(Invalid::Missing(key)); }
            if s.chars().count() > max { return Err(Invalid::Long(key.into())); }
            Ok(s.clone())
        }
        Some(_) => Err(Invalid::Type(key.into())),
    }
}

fn one(v: &Value) -> Result<Event, Invalid> {
    let m = v.as_object().ok_or(Invalid::Json)?;
    match m.get("v") {
        Some(Value::Number(n)) if n.as_u64() == Some(1) => {}
        Some(_) => return Err(Invalid::Version),
        None => return Err(Invalid::Missing("v")),
    }
    let n = required_str(m, "n", 32)?;
    if !good_name(&n) { return Err(Invalid::Name); }
    let id = required_str(m, "id", 32)?;
    let p = required_str(m, "p", 2048)?;
    let mut fields = BTreeMap::new();
    for (k, val) in m {
        if let Some(kind) = kind_of(k) {
            check_value(k, kind, val)?;
            fields.insert(k.clone(), val.clone());
        }
    }
    Ok(Event { v: 1, n, id, p, fields, s: None })
}

/// A POST body: one event or an array of at most `MAX_EVENTS`.
pub fn validate_json(body: &[u8]) -> Result<Vec<Event>, Invalid> {
    let text = std::str::from_utf8(body).map_err(|_| Invalid::Json)?;
    let value: Value = serde_json::from_str(text).map_err(|_| Invalid::Json)?;
    match value {
        Value::Array(items) => {
            if items.len() > MAX_EVENTS { return Err(Invalid::TooMany); }
            items.iter().map(one).collect()
        }
        other => Ok(vec![one(&other)?]),
    }
}

/// A GET query string: the same keys as text, coerced by the key table.
pub fn validate_query(query: &str) -> Result<Vec<Event>, Invalid> {
    let pairs: Vec<(String, String)> = serde_urlencoded::from_str(query).map_err(|_| Invalid::Json)?;
    let mut m = Map::new();
    for (k, raw) in pairs {
        let value = match k.as_str() {
            "v" => Value::Number(raw.parse::<u64>().map_err(|_| Invalid::Version)?.into()),
            "n" | "id" | "p" => Value::String(raw),
            key => match kind_of(key) {
                None => continue,
                Some(Kind::Str(_)) => Value::String(raw),
                Some(Kind::Int) => raw.parse::<i64>().map(Value::from).map_err(|_| Invalid::Type(key.into()))?,
                Some(Kind::Num) => raw.parse::<f64>().ok().and_then(serde_json::Number::from_f64).map(Value::Number).ok_or_else(|| Invalid::Type(key.into()))?,
                Some(Kind::Bool) => match raw.as_str() { "true" => Value::Bool(true), "false" => Value::Bool(false), _ => return Err(Invalid::Type(key.into())) },
                Some(Kind::Payload) => serde_json::from_str(&raw).map_err(|_| Invalid::Type(key.into()))?,
            },
        };
        m.insert(k, value);
    }
    Ok(vec![one(&Value::Object(m))?])
}
