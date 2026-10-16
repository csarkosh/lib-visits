//! The report types of the wire spec's section 6 and the bytes they put on the
//! wire. Every field is public; `to_bytes` and `to_query` build the JSON by hand
//! and run it through `visits_core::app`, so nothing the collector would refuse
//! ever leaves this crate.

use serde_json::{Map, Value};
use std::collections::BTreeMap;
use visits_core::{validate_check, validate_report, Invalid};

/// The most events a session report carries (section 6.4).
const MAX_EVENTS: usize = 200;

/// The operating system a program runs on.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Os {
    Macos,
    Linux,
    Windows,
}

impl Os {
    /// The wire spelling.
    pub fn as_str(self) -> &'static str {
        match self {
            Os::Macos => "macos",
            Os::Linux => "linux",
            Os::Windows => "windows",
        }
    }
}

/// The processor architecture a program was built for.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Arch {
    Arm64,
    X86_64,
}

impl Arch {
    /// The wire spelling.
    pub fn as_str(self) -> &'static str {
        match self {
            Arch::Arm64 => "arm64",
            Arch::X86_64 => "x86_64",
        }
    }
}

/// How the program was distributed. A `Named` channel (`tap`, `direct`) is
/// checked against the name-with-dash pattern when the report is written.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum Channel {
    Release,
    Source,
    Named(String),
}

impl Channel {
    /// The wire spelling: the channel's plain name.
    pub fn as_str(&self) -> &str {
        match self {
            Channel::Release => "release",
            Channel::Source => "source",
            Channel::Named(name) => name,
        }
    }

    /// Whether this is a source build, spelled as the variant or as the name.
    pub fn is_source(&self) -> bool {
        self.as_str() == "source"
    }
}

/// The keys every report and check carries (section 6.2), less `v` and `k`.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Envelope {
    pub app: String,
    pub ver: String,
    pub bin: String,
    pub os: Os,
    pub osv: u32,
    pub arch: Arch,
    pub ch: Channel,
}

impl Envelope {
    /// The envelope's keys in the spec's order, `v` first and `k` last, as
    /// strings, which is how a check's query carries them.
    fn pairs(&self, k: &str) -> [(&'static str, String); 9] {
        [
            ("v", "2".to_string()),
            ("app", self.app.clone()),
            ("ver", self.ver.clone()),
            ("bin", self.bin.clone()),
            ("os", self.os.as_str().to_string()),
            ("osv", self.osv.to_string()),
            ("arch", self.arch.as_str().to_string()),
            ("ch", self.ch.as_str().to_string()),
            ("k", k.to_string()),
        ]
    }

    /// The envelope as the JSON object a report starts from.
    pub(crate) fn to_map(&self, k: &str) -> Map<String, Value> {
        let mut m = Map::new();
        m.insert("v".into(), Value::from(2));
        m.insert("app".into(), Value::from(self.app.as_str()));
        m.insert("ver".into(), Value::from(self.ver.as_str()));
        m.insert("bin".into(), Value::from(self.bin.as_str()));
        m.insert("os".into(), Value::from(self.os.as_str()));
        m.insert("osv".into(), Value::from(self.osv));
        m.insert("arch".into(), Value::from(self.arch.as_str()));
        m.insert("ch".into(), Value::from(self.ch.as_str()));
        m.insert("k".into(), Value::from(k));
        m
    }
}

/// A value in an event's `d`.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum DValue {
    Str(String),
    Int(i64),
    Bool(bool),
}

impl DValue {
    fn to_value(&self) -> Value {
        match self {
            DValue::Str(s) => Value::from(s.as_str()),
            DValue::Int(i) => Value::from(*i),
            DValue::Bool(b) => Value::from(*b),
        }
    }
}

/// One event of a session: its name, seconds since launch, and its data.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Event {
    pub n: String,
    pub t: u64,
    pub d: BTreeMap<String, DValue>,
}

impl Event {
    /// The event as its JSON object, with an empty `d` left out.
    pub(crate) fn to_value(&self) -> Value {
        let mut m = Map::new();
        m.insert("n".into(), Value::from(self.n.as_str()));
        m.insert("t".into(), Value::from(self.t));
        if !self.d.is_empty() {
            let d = self.d.iter().map(|(k, v)| (k.clone(), v.to_value())).collect();
            m.insert("d".into(), Value::Object(d));
        }
        Value::Object(m)
    }
}

/// How a session ended.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum End {
    Quit,
    Lost,
    Unknown,
}

impl End {
    /// The wire spelling.
    pub fn as_str(self) -> &'static str {
        match self {
            End::Quit => "quit",
            End::Lost => "lost",
            End::Unknown => "unknown",
        }
    }
}

/// A session report (section 6.4).
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Session {
    pub env: Envelope,
    pub sid: String,
    pub len: u64,
    pub end: End,
    pub iw: i64,
    pub rd: i64,
    pub focus: BTreeMap<String, u64>,
    pub ev: Vec<Event>,
    pub ev_dropped: u64,
    pub ev_rejected: u64,
}

impl Session {
    /// Adds an event while there is room for one; past 200, the event is
    /// counted in `ev_dropped` instead, so the report stays within the wire's cap.
    pub fn push(&mut self, ev: Event) {
        if self.ev.len() < MAX_EVENTS {
            self.ev.push(ev);
        } else {
            self.ev_dropped += 1;
        }
    }

    /// The exact bytes that go on the wire, refused with the collector's own
    /// reason when any field breaks section 6.
    pub fn to_bytes(&self) -> Result<Vec<u8>, Invalid> {
        let mut m = self.env.to_map("session");
        m.insert("sid".into(), Value::from(self.sid.as_str()));
        m.insert("len".into(), Value::from(self.len));
        m.insert("end".into(), Value::from(self.end.as_str()));
        m.insert("iw".into(), Value::from(self.iw));
        m.insert("rd".into(), Value::from(self.rd));
        let focus = self.focus.iter().map(|(k, v)| (k.clone(), Value::from(*v))).collect();
        m.insert("focus".into(), Value::Object(focus));
        m.insert("ev".into(), Value::Array(self.ev.iter().map(Event::to_value).collect()));
        m.insert("ev_dropped".into(), Value::from(self.ev_dropped));
        m.insert("ev_rejected".into(), Value::from(self.ev_rejected));
        checked(m)
    }
}

/// What ended the process.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CrashKind {
    Panic,
    Signal,
}

impl CrashKind {
    /// The wire spelling.
    pub fn as_str(self) -> &'static str {
        match self {
            CrashKind::Panic => "panic",
            CrashKind::Signal => "signal",
        }
    }
}

/// How long the process ran before it crashed, in the spec's buckets.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Uptime {
    Lt1m,
    Lt10m,
    Lt1h,
    Lt1d,
    Ge1d,
}

impl Uptime {
    /// The wire spelling.
    pub fn as_str(self) -> &'static str {
        match self {
            Uptime::Lt1m => "<1m",
            Uptime::Lt10m => "<10m",
            Uptime::Lt1h => "<1h",
            Uptime::Lt1d => "<1d",
            Uptime::Ge1d => ">=1d",
        }
    }
}

/// The memory pressure the system reported at the crash.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Memory {
    Normal,
    Warn,
    Critical,
    Unknown,
}

impl Memory {
    /// The wire spelling.
    pub fn as_str(self) -> &'static str {
        match self {
            Memory::Normal => "normal",
            Memory::Warn => "warn",
            Memory::Critical => "critical",
            Memory::Unknown => "unknown",
        }
    }
}

/// A crash report (section 6.5).
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Crash {
    pub env: Envelope,
    pub inc: String,
    pub ck: CrashKind,
    pub commit: String,
    pub pk: String,
    pub mh: Option<String>,
    pub file: String,
    pub line: u32,
    pub frames: Vec<(u32, u64)>,
    pub images: Vec<(String, String, u64)>,
    pub sig: Option<i32>,
    pub up: Uptime,
    pub mem: Option<Memory>,
}

impl Crash {
    /// The exact bytes that go on the wire, refused with the collector's own
    /// reason when any field breaks section 6.
    pub fn to_bytes(&self) -> Result<Vec<u8>, Invalid> {
        let mut m = self.env.to_map("crash");
        m.insert("inc".into(), Value::from(self.inc.as_str()));
        m.insert("ck".into(), Value::from(self.ck.as_str()));
        m.insert("commit".into(), Value::from(self.commit.as_str()));
        m.insert("pk".into(), Value::from(self.pk.as_str()));
        if let Some(mh) = &self.mh {
            m.insert("mh".into(), Value::from(mh.as_str()));
        }
        m.insert("file".into(), Value::from(self.file.as_str()));
        m.insert("line".into(), Value::from(self.line));
        let frames = self.frames.iter().map(|&(image, offset)| Value::from(vec![Value::from(image), Value::from(offset)]));
        m.insert("frames".into(), Value::Array(frames.collect()));
        let images = self
            .images
            .iter()
            .map(|(name, uuid, size)| Value::from(vec![Value::from(name.as_str()), Value::from(uuid.as_str()), Value::from(*size)]));
        m.insert("images".into(), Value::Array(images.collect()));
        if let Some(sig) = self.sig {
            m.insert("sig".into(), Value::from(sig));
        }
        m.insert("up".into(), Value::from(self.up.as_str()));
        if let Some(mem) = self.mem {
            m.insert("mem".into(), Value::from(mem.as_str()));
        }
        checked(m)
    }
}

/// An update check (section 6.3), sent as a GET query.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Check {
    pub env: Envelope,
    pub rd: i64,
}

impl Check {
    /// The GET query, its keys in the envelope's order and `rd` last, refused
    /// with the collector's own reason when any value breaks section 6.
    pub fn to_query(&self) -> Result<String, Invalid> {
        let rd = self.rd.to_string();
        let pairs = self.env.pairs("check");
        let query = pairs
            .iter()
            .map(|(k, v)| (*k, v.as_str()))
            .chain([("rd", rd.as_str())])
            .map(|(k, v)| format!("{k}={}", encode(v)))
            .collect::<Vec<_>>()
            .join("&");
        validate_check(&query)?;
        Ok(query)
    }
}

/// Serializes a report and validates the very bytes it returns.
fn checked(m: Map<String, Value>) -> Result<Vec<u8>, Invalid> {
    // Serializing a map of strings, numbers and booleans cannot fail.
    let bytes = serde_json::to_vec(&Value::Object(m)).map_err(|_| Invalid::Json)?;
    validate_report(&bytes)?;
    Ok(bytes)
}

/// Percent-encodes every byte outside the URL's unreserved set, so that a `+`
/// in a version's build metadata is not read back as a space.
fn encode(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        if b.is_ascii_alphanumeric() || matches!(b, b'-' | b'.' | b'_' | b'~') {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}
