//! The wire spec's section 6 as code: app reports and checks. Every key has a
//! type, a pattern and a cap, and a key outside the tables, at any depth, refuses
//! the whole report.

use crate::clock::{rfc3339, rfc3339_ms};
use crate::validate::Invalid;
use serde::Serialize;
use serde_json::{Map, Value};

/// A report or a check that passed section 6, and the collector's decoration
/// once added. It has no `Deserialize`: `fields` is built only by the walk below.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct AppReport {
    #[serde(flatten)]
    pub fields: Map<String, Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub s: Option<AppDecoration>,
    /// The time of receipt, RFC 3339 with milliseconds (section 6.8).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub time: Option<String>,
}

/// What the collector adds to a report (section 6.7), and nothing else.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct AppDecoration {
    pub sv: u8,
    pub at: String,
}

/// The character classes of section 6.6, matched by hand.
#[derive(Clone, Copy)]
enum Pat {
    /// `^[a-z][a-z0-9_]*$`, its length set by the cap.
    Name,
    /// `^[a-z][a-z0-9_-]*$`, its length set by the cap.
    NameDash,
    /// `^[0-9]+\.[0-9]+\.[0-9]+([-+][0-9A-Za-z.-]+)?$`
    Version,
    /// `^[A-Za-z0-9_.:/-]*$`
    Value,
    /// `^[A-Za-z0-9_./-]*$`
    Path,
    /// `^[A-Za-z0-9_.-]+$`
    ImageName,
}

#[derive(Clone, Copy)]
enum Kind {
    /// The integer 2; anything else is `Version`.
    Two,
    Str { cap: usize, pat: Pat },
    Enum(&'static [&'static str]),
    Hex { min: usize, max: usize },
    /// An integer from `min` to `max`; `u64::MAX` as `max` is no cap.
    Int { min: i64, max: u64 },
    /// Days as a whole number of weeks: an integer from 0 to `MAX_DAYS` and a multiple of 7.
    Week,
    Focus,
    Events,
    Data,
    Frames,
    Images,
}

struct Field {
    key: &'static str,
    /// The dotted path the key is named by in an error, without an index.
    path: &'static str,
    kind: Kind,
    required: bool,
}

const fn must(key: &'static str, kind: Kind) -> Field {
    Field { key, path: key, kind, required: true }
}

const fn may(key: &'static str, kind: Kind) -> Field {
    Field { key, path: key, kind, required: false }
}

/// The most events a session report carries.
pub const MAX_EVENTS: usize = 200;
/// The most keys an event's `d` carries.
pub const MAX_KEYS: usize = 6;
/// The most surfaces a session's `focus` carries.
pub const MAX_FOCUS: usize = 16;
/// The longest name: an event's `n`, a `d` key, a `focus` key, `app`, `bin` and `ch`.
pub const MAX_NAME: usize = 32;
/// The longest `d` string value.
pub const MAX_VALUE: usize = 64;
const MAX_FRAMES: usize = 64;
const MAX_IMAGES: usize = 32;

/// The greatest `osv`, an OS major version.
pub const MAX_OSV: u64 = 999;
/// The greatest count of days: `rd` and `iw`.
pub const MAX_DAYS: u64 = 100_000;
/// The greatest count of seconds, 31 days: a session's `len`, an event's `t` and a focus total.
pub const MAX_SECS: u64 = 31 * 86_400;
/// The greatest source line of a crash.
pub const MAX_LINE: u64 = 10_000_000;
/// The greatest signal number of a crash.
pub const MAX_SIG: u64 = 255;
/// The greatest `ev_dropped` and `ev_rejected`.
pub const MAX_COUNT: u64 = 1_000_000;

const NAME: Kind = Kind::Str { cap: MAX_NAME, pat: Pat::Name };
const NAME_DASH: Kind = Kind::Str { cap: MAX_NAME, pat: Pat::NameDash };
const OSV: Kind = Kind::Int { min: 0, max: MAX_OSV };
const SECS: Kind = Kind::Int { min: 0, max: MAX_SECS };
const COUNT: Kind = Kind::Int { min: 0, max: MAX_COUNT };
const DAYS: Kind = Kind::Int { min: -1, max: MAX_DAYS };

const ENVELOPE: &[Field] = &[
    must("v", Kind::Two),
    must("app", NAME_DASH),
    must("ver", Kind::Str { cap: 32, pat: Pat::Version }),
    must("bin", NAME_DASH),
    must("os", Kind::Enum(&["macos", "linux", "windows"])),
    must("osv", OSV),
    must("arch", Kind::Enum(&["arm64", "x86_64"])),
    must("ch", NAME_DASH),
    must("k", Kind::Enum(&["check", "session", "crash"])),
];

const CHECK: &[Field] = &[must("rd", DAYS)];

const SESSION: &[Field] = &[
    must("sid", Kind::Hex { min: 16, max: 32 }),
    must("len", SECS),
    must("end", Kind::Enum(&["quit", "lost", "unknown"])),
    must("iw", Kind::Week),
    must("rd", DAYS),
    must("focus", Kind::Focus),
    must("ev", Kind::Events),
    may("ev_dropped", COUNT),
    may("ev_rejected", COUNT),
];

const EVENT: &[Field] = &[
    Field { key: "n", path: "ev.n", kind: NAME, required: true },
    Field { key: "t", path: "ev.t", kind: SECS, required: true },
    Field { key: "d", path: "ev.d", kind: Kind::Data, required: false },
];

const CRASH: &[Field] = &[
    must("inc", Kind::Hex { min: 16, max: 32 }),
    must("ck", Kind::Enum(&["panic", "signal"])),
    must("commit", Kind::Hex { min: 7, max: 40 }),
    must("pk", Kind::Str { cap: 64, pat: Pat::Name }),
    may("mh", Kind::Hex { min: 64, max: 64 }),
    must("file", Kind::Str { cap: 256, pat: Pat::Path }),
    must("line", Kind::Int { min: 0, max: MAX_LINE }),
    must("frames", Kind::Frames),
    must("images", Kind::Images),
    may("sig", Kind::Int { min: 0, max: MAX_SIG }),
    must("up", Kind::Enum(&["<1m", "<10m", "<1h", "<1d", ">=1d"])),
    may("mem", Kind::Enum(&["normal", "warn", "critical", "unknown"])),
];

fn name(b: &[u8], dash: bool) -> bool {
    matches!(b.first(), Some(c) if c.is_ascii_lowercase())
        && b[1..].iter().all(|&c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == b'_' || (dash && c == b'-'))
}

fn version(b: &[u8]) -> bool {
    let mut i = 0;
    for part in 0..3 {
        if part > 0 {
            if b.get(i) != Some(&b'.') { return false; }
            i += 1;
        }
        let start = i;
        while b.get(i).is_some_and(u8::is_ascii_digit) { i += 1; }
        if i == start { return false; }
    }
    match b.get(i) {
        None => true,
        Some(b'-' | b'+') => i + 1 < b.len() && b[i + 1..].iter().all(|&c| c.is_ascii_alphanumeric() || c == b'.' || c == b'-'),
        Some(_) => false,
    }
}

fn matches(pat: Pat, s: &str) -> bool {
    let b = s.as_bytes();
    let class = |extra: &[u8]| b.iter().all(|c| c.is_ascii_alphanumeric() || extra.contains(c));
    match pat {
        Pat::Name => name(b, false),
        Pat::NameDash => name(b, true),
        Pat::Version => version(b),
        Pat::Value => class(b"_.:/-"),
        Pat::Path => class(b"_./-"),
        Pat::ImageName => !b.is_empty() && class(b"_.-"),
    }
}

fn hex(s: &str, min: usize, max: usize) -> bool {
    (min..=max).contains(&s.len()) && s.bytes().all(|c| c.is_ascii_digit() || (b'a'..=b'f').contains(&c))
}

/// Whether `s` is a name as section 6.6 has it (`^[a-z][a-z0-9_]{0,31}$`): an
/// event's `n`, a `d` key or a `focus` key. The client crate checks a program's
/// allowlist with it, so the allowlist cannot admit what the walk would refuse.
pub fn is_name(s: &str) -> bool {
    s.len() <= MAX_NAME && name(s.as_bytes(), false)
}

/// Whether `s` is a `d` string value as section 6.6 has it (`^[A-Za-z0-9_.:/-]*$`, cap 64).
pub fn is_value(s: &str) -> bool {
    s.len() <= MAX_VALUE && matches(Pat::Value, s)
}

/// Whether `s` is lowercase hex of `min` to `max` characters, as a `sid` or an `inc` is.
pub fn is_hex(s: &str, min: usize, max: usize) -> bool {
    hex(s, min, max)
}

/// A child's path: `ev` and `x` make `ev.x`.
fn join(prefix: &str, key: &str) -> String {
    if prefix.is_empty() { key.to_string() } else { format!("{prefix}.{key}") }
}

fn string<'a>(v: &'a Value, path: &str) -> Result<&'a str, Invalid> {
    v.as_str().ok_or_else(|| Invalid::Type(path.into()))
}

fn int(v: &Value, min: i64, max: u64, path: &str) -> Result<(), Invalid> {
    match v {
        // A whole number `>= 0` is above every minimum, so only the cap is checked.
        Value::Number(n) if n.is_u64() => {
            if n.as_u64().is_some_and(|u| u <= max) { Ok(()) } else { Err(Invalid::Pattern(path.into())) }
        }
        Value::Number(n) => match n.as_i64() {
            Some(i) if i >= min => Ok(()),
            Some(_) => Err(Invalid::Pattern(path.into())),
            None => Err(Invalid::Type(path.into())),
        },
        _ => Err(Invalid::Type(path.into())),
    }
}

fn array<'a>(v: &'a Value, cap: usize, path: &str) -> Result<&'a [Value], Invalid> {
    let items = v.as_array().ok_or_else(|| Invalid::Type(path.into()))?;
    if items.len() > cap { return Err(Invalid::Keys(path.into())); }
    Ok(items)
}

fn object<'a>(v: &'a Value, cap: usize, path: &str) -> Result<&'a Map<String, Value>, Invalid> {
    let m = v.as_object().ok_or_else(|| Invalid::Type(path.into()))?;
    if m.len() > cap { return Err(Invalid::Keys(path.into())); }
    Ok(m)
}

/// A key of `focus` or of an event's `d`, and the path its value is named by.
fn name_key(path: &str, key: &str) -> Result<String, Invalid> {
    let child = join(path, key);
    if is_name(key) { Ok(child) } else { Err(Invalid::Pattern(child)) }
}

/// A tuple of `frames` or `images`: an array of exactly `len` items.
fn tuple<'a>(v: &'a Value, len: usize, path: &str) -> Result<&'a [Value], Invalid> {
    match v {
        Value::Array(a) if a.len() == len => Ok(a),
        Value::Array(_) => Err(Invalid::Pattern(path.into())),
        _ => Err(Invalid::Type(path.into())),
    }
}

fn check_value(kind: Kind, v: &Value, path: &str) -> Result<(), Invalid> {
    match kind {
        Kind::Two => if v.as_u64() == Some(2) { Ok(()) } else { Err(Invalid::Version) },
        Kind::Str { cap, pat } => {
            let s = string(v, path)?;
            // Every pattern is ASCII, so a string that matches has as many bytes as characters.
            if s.len() <= cap && matches(pat, s) { Ok(()) } else { Err(Invalid::Pattern(path.into())) }
        }
        Kind::Enum(set) => if set.contains(&string(v, path)?) { Ok(()) } else { Err(Invalid::Pattern(path.into())) },
        Kind::Hex { min, max } => if hex(string(v, path)?, min, max) { Ok(()) } else { Err(Invalid::Pattern(path.into())) },
        Kind::Int { min, max } => int(v, min, max, path),
        Kind::Week => {
            int(v, 0, MAX_DAYS, path)?;
            // `int` with a minimum of 0 leaves only whole numbers that fit a `u64`.
            if v.as_u64().is_some_and(|w| w % 7 == 0) { Ok(()) } else { Err(Invalid::Pattern(path.into())) }
        }
        Kind::Focus => {
            for (k, secs) in object(v, MAX_FOCUS, path)? {
                int(secs, 0, MAX_SECS, &name_key(path, k)?)?;
            }
            Ok(())
        }
        Kind::Events => {
            for e in array(v, MAX_EVENTS, path)? {
                walk(e.as_object().ok_or_else(|| Invalid::Type(path.into()))?, &[EVENT], path)?;
            }
            Ok(())
        }
        Kind::Data => {
            for (k, value) in object(v, MAX_KEYS, path)? {
                let child = name_key(path, k)?;
                match value {
                    Value::String(s) if is_value(s) => {}
                    Value::String(_) => return Err(Invalid::Pattern(child)),
                    // A payload integer has no minimum (a delta can be negative) but must fit an `i64`.
                    Value::Number(n) if n.is_i64() => {}
                    Value::Number(n) if n.is_u64() => return Err(Invalid::Pattern(child)),
                    Value::Bool(_) => {}
                    _ => return Err(Invalid::Type(child)),
                }
            }
            Ok(())
        }
        Kind::Frames => {
            for pair in array(v, MAX_FRAMES, path)? {
                for n in tuple(pair, 2, path)? {
                    int(n, 0, u64::MAX, path)?;
                }
            }
            Ok(())
        }
        Kind::Images => {
            for triple in array(v, MAX_IMAGES, path)? {
                let t = tuple(triple, 3, path)?;
                check_value(Kind::Str { cap: 64, pat: Pat::ImageName }, &t[0], "images.name")?;
                check_value(Kind::Hex { min: 32, max: 32 }, &t[1], "images.uuid")?;
                int(&t[2], 0, u64::MAX, "images.size")?;
            }
            Ok(())
        }
    }
}

fn known(tables: &[&[Field]], key: &str) -> Option<Kind> {
    tables.iter().flat_map(|t| t.iter()).find(|f| f.key == key).map(|f| f.kind)
}

/// Checks every key of `obj` against `tables` and copies the checked keys into a
/// fresh map: a key outside the tables is `Unknown`, whatever its value.
fn walk(obj: &Map<String, Value>, tables: &[&[Field]], prefix: &str) -> Result<Map<String, Value>, Invalid> {
    if let Some(key) = obj.keys().find(|k| known(tables, k).is_none()) {
        return Err(Invalid::Unknown(join(prefix, key)));
    }
    let mut out = Map::new();
    for f in tables.iter().flat_map(|t| t.iter()) {
        match obj.get(f.key) {
            None if f.required => return Err(Invalid::Missing(f.path)),
            None => {}
            Some(v) => {
                check_value(f.kind, v, f.path)?;
                out.insert(f.key.to_string(), v.clone());
            }
        }
    }
    Ok(out)
}

/// One report or check as a JSON object. `v` is read first and `k` second, since
/// `k` chooses the table; `ch == "source"` is read last, so a malformed report
/// from a source build is still `400` and only a valid one is `403`.
fn report(obj: &Map<String, Value>, is_check: bool, allow_source: bool) -> Result<AppReport, Invalid> {
    match obj.get("v") {
        None => return Err(Invalid::Missing("v")),
        Some(v) => check_value(Kind::Two, v, "v")?,
    }
    let k = match obj.get("k") {
        None => return Err(Invalid::Missing("k")),
        Some(k) => string(k, "k")?,
    };
    let table = match k {
        "check" => CHECK,
        "session" => SESSION,
        "crash" => CRASH,
        _ => return Err(Invalid::Pattern("k".into())),
    };
    if (k == "check") != is_check { return Err(Invalid::Kind); }
    let fields = walk(obj, &[ENVELOPE, table], "")?;
    if !allow_source && fields.get("ch").and_then(Value::as_str) == Some("source") { return Err(Invalid::Channel); }
    Ok(AppReport { fields, s: None, time: None })
}

/// A POST body: exactly one report object. An array, or anything that is not
/// an object, is malformed; a check (`k: "check"`) is the wrong kind. A report
/// from a source build is `Invalid::Channel`, which is the collector's `403`.
pub fn validate_report(body: &[u8]) -> Result<AppReport, Invalid> {
    parse_report(body, false)
}

/// `validate_report` without its last check: a report from a source build is
/// `Ok` when everything else is valid. This is the client side's entry, for
/// reading back a report the program wrote itself; the collector keeps
/// `validate_report` and so keeps refusing the source channel.
pub fn validate_report_allowing_source(body: &[u8]) -> Result<AppReport, Invalid> {
    parse_report(body, true)
}

fn parse_report(body: &[u8], allow_source: bool) -> Result<AppReport, Invalid> {
    let text = std::str::from_utf8(body).map_err(|_| Invalid::Json)?;
    match serde_json::from_str::<Value>(text).map_err(|_| Invalid::Json)? {
        Value::Object(obj) => report(&obj, false, allow_source),
        _ => Err(Invalid::Json),
    }
}

/// The text of an integer key, read as JSON would read the same number: only
/// `-?[0-9]+`, as an `i64` or else a `u64`. Anything else stays a string, and
/// the walk calls it a bad type.
fn query_int(raw: String) -> Value {
    let digits = raw.strip_prefix('-').unwrap_or(&raw);
    if digits.is_empty() || !digits.bytes().all(|c| c.is_ascii_digit()) { return Value::String(raw); }
    match (raw.parse::<i64>(), raw.parse::<u64>()) {
        (Ok(i), _) => Value::from(i),
        (_, Ok(u)) => Value::from(u),
        _ => Value::String(raw),
    }
}

/// A GET query: the envelope and `rd`, with `k` equal to `check`. A known key
/// given twice is a bad value, so the last one never silently wins; an unknown
/// key, given once or twice, is left for the walk to name.
pub fn validate_check(query: &str) -> Result<AppReport, Invalid> {
    let pairs: Vec<(String, String)> = serde_urlencoded::from_str(query).map_err(|_| Invalid::Json)?;
    let mut obj = Map::new();
    for (key, raw) in pairs {
        let kind = known(&[ENVELOPE, CHECK], &key);
        if kind.is_some() && obj.contains_key(&key) { return Err(Invalid::Pattern(key)); }
        let value = match kind {
            Some(Kind::Two | Kind::Int { .. }) => query_int(raw),
            _ => Value::String(raw),
        };
        obj.insert(key, value);
    }
    report(&obj, true, false)
}

/// Adds `s` (`sv` 2 and the time of receipt in whole seconds) and `time` (the
/// same instant with milliseconds), and nothing about the request.
pub fn decorate_report(mut r: AppReport, unix_millis: u64) -> AppReport {
    r.s = Some(AppDecoration { sv: 2, at: rfc3339(unix_millis / 1000) });
    r.time = Some(rfc3339_ms(unix_millis));
    r
}

fn text<'a>(r: &'a AppReport, key: &str) -> &'a str {
    r.fields.get(key).and_then(Value::as_str).unwrap_or("")
}

/// The report's `app`.
pub fn app_of(r: &AppReport) -> &str {
    text(r, "app")
}

/// The report's `ch`.
pub fn channel_of(r: &AppReport) -> &str {
    text(r, "ch")
}

/// The report's `k`: `check`, `session` or `crash`.
pub fn kind_of(r: &AppReport) -> &str {
    text(r, "k")
}
