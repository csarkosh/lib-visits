use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// One event as the wire carries it: the four required keys, every other
/// known key in `fields`, and the collector's decoration once added.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Event {
    pub v: u8,
    pub n: String,
    pub id: String,
    pub p: String,
    #[serde(flatten)]
    pub fields: BTreeMap<String, serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub s: Option<Decoration>,
    /// The event's time, RFC 3339 with milliseconds (spec section 5): the
    /// client's `ts` when it is near the time of receipt, else that time. Never
    /// read from the wire.
    #[serde(skip_deserializing, skip_serializing_if = "Option::is_none")]
    pub time: Option<String>,
}

/// What the collector adds. `None` fields are left out of the line.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Decoration {
    pub sv: u8,
    pub at: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub visit: Option<String>,
    pub host: String,
    pub bn: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bv: Option<u32>,
    pub os: String,
    pub bot: bool,
    pub rk: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rp: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rh: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub lg: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cc: Option<String>,
}
