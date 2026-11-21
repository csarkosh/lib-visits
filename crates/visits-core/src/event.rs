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
    /// The visitor across days, only under `--visitor`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub vid: Option<String>,
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
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rg: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ct: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub la: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub lo: Option<f64>,
    /// The network the address is on: its autonomous system's number and its owner's name (an
    /// ISP, a company, a cloud), from an ASN database (`--asn`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub an: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ao: Option<String>,
    /// The kind of network: `tor`, `cloud`, `vpn` or `relay`; absent for any other.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub nk: Option<String>,
    /// Chromium's network hints, when the page asked for them with `Accept-CH`: the effective
    /// connection type, the round trip in ms, the downlink in Mbit/s, and Save-Data when on.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ect: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rtt: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub dl: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sd: Option<bool>,
}
