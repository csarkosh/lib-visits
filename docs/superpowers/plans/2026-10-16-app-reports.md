# App reports Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** The collector takes typed reports and update checks from installed programs on a second route, with no identifier and no address in what it writes; a new pure crate, `visits-app`, gives a program the client half: the report types, the consent mode, the journal, the sealing, the retry queue and the daily check.

**Architecture:** The wire format gains section 6 and fixtures under `spec/fixtures/app/`, written first. `visits-core` gains `app.rs`: the schema as code, used by the collector to validate what arrives and by the client crate to validate what it writes. `visits` gains two handlers beside `beacon` and the `--app` flags. `visits-app` is a third crate with no network and no clock of its own: every function takes `now` as unix seconds and a `Transport` the program implements.

**Tech Stack:** Rust stable, edition 2021; `serde`, `serde_json`; `toml` in `visits-app` only (the allowlist file); no new dependency in the binary.

**Spec:** `docs/superpowers/specs/2026-10-16-app-reports-design.md`

## Global Constraints

- The binary's runtime dependencies do not grow: `axum`, `tokio`, `serde`, `serde_json`, `serde_urlencoded`, `hmac`, `sha2`, `clap`, `anyhow` (and `maxminddb` under `geoip`). `visits-app` adds `toml` to itself only.
- Nothing about a report's request is written or printed: no address, no visit hash, no User-Agent, no country; `--log-ip` does not apply to the report routes. The decoration of a report is `sv` and `at` only.
- Every string field has a pattern (an enum, a hex length, or a character class) and a cap; there is no free-text string anywhere in section 6. A report with an unknown key, at any depth, is refused whole (`400`), unlike a beacon, whose unknown keys are dropped. `Content-Type` is not checked, and the SPEC says so.
- A report whose `ch` is `source` answers `403` and is never written; the client crate never uploads one.
- Every answer carries `cache-control: no-store`; a check's redirect is `302` with `Location`.
- The web routes and their tests are unchanged byte for byte in behaviour; `cargo test --workspace` stays green at every task.
- The client crate never calls its `Transport` except from `Queue::drain`; nothing in it spawns, sleeps or reads the clock.
- No em-dashes in any file. Conventional Commits, a short body saying why, paths staged explicitly.
- Build hygiene on this machine: `nice -n 15 cargo ... -j 3`, no `--release`, one cargo process at a time, nothing heavy in the background; never commit a `.cargo/` folder.

## Shared definitions (every task uses these spellings)

**Required and optional keys per kind.** Envelope, all required: `v` (`2`), `app`, `ver`, `bin`, `os`, `osv`, `arch`, `ch`, `k`. `check` (GET only): `rd` required. `session`: `sid`, `len`, `end`, `iw`, `rd`, `focus`, `ev` required; `ev_dropped`, `ev_rejected` optional integers `>= 0`. An event: `n`, `t` required; `d` optional. `crash`: `inc`, `ck`, `commit`, `pk`, `file`, `line`, `frames`, `images`, `up` required; `mh`, `sig`, `mem` optional. A POST body with `k: "check"` is `400`; a GET check with any other `k` is `400`.

**Value rules.** Enums: `os` in `macos|linux|windows`; `arch` in `arm64|x86_64`; `k` in `check|session|crash`; `end` in `quit|lost|unknown`; `ck` in `panic|signal`; `up` in `<1m|<10m|<1h|<1d|>=1d`; `mem` in `normal|warn|critical|unknown`. Names (`^[a-z][a-z0-9_]{0,31}$`): `n`, `d` keys, `focus` keys, `pk` (cap 64). Names with dashes (`^[a-z][a-z0-9_-]{0,31}$`): `app`, `bin`, `ch`. Hex: `sid`, `inc` 16 to 32 chars; `commit` 7 to 40; `mh` 64; image uuid 32. `ver`: `^[0-9]+\.[0-9]+\.[0-9]+([-+][0-9A-Za-z.-]+)?$`, cap 32. Values (`^[A-Za-z0-9_.:/-]*$`, cap 64): `d` string values. Paths (`^[A-Za-z0-9_./-]*$`, cap 256): `file`. Image names: `^[A-Za-z0-9_.-]{1,64}$`. Integers carry a minimum: `rd >= -1`; `osv`, `len`, `iw`, `t`, `line`, `sig`, `ev_dropped`, `ev_rejected`, focus values, frame pairs and image sizes `>= 0`. Counts: `ev` at most 200, `d` at most 6 keys, `focus` at most 16, `frames` at most 64 pairs of two integers, `images` at most 32 triples.

**Error spelling.** `Invalid` gains `Unknown(String)` ("unknown key: <path>"), `Pattern(String)` ("bad value: <path>"), `Channel` ("source channel"), `Kind` ("wrong kind for this route"). A path is dotted with no index: `ev.d.x`, `focus.terminal`, `images.uuid`, `frames`. More than 200 events, more than 6 `d` keys or more than 16 `focus` keys is `Keys(<path>)` (`400`), never `TooMany` (which the beacon route maps to `413`). A duplicate key in a check query is `Pattern(<key>)`; the last value never silently wins. A name that fails its pattern is `Pattern(<path>)` (`Pattern("ev.n")` for an event name), never the beacon's `Name`.

**Mode reasons** (`&'static str`): `"source build"`, `"DO_NOT_TRACK"`, `"telemetry off"`, `"telemetry log"`, `"CI"`, `"not interactive"`.

**Who saves `State`.** `seal`, `drain` and `checked` each take `&mut State` and the caller saves it with `State::save` once after the call; the crate never writes `state.toml` on its own.

## Review Focus

1. A report with a key the schema does not know, at the top level, in an event's `d`, in `focus`, or a top-level `s` or `time`, answers `400` and nothing is written. Pinned in Task 2 and Task 3a.
2. A check whose `rd` is missing, below `-1`, or not an integer answers `400` and is not written; a valid one is written before the redirect is answered, so a client that does not follow redirects still counts. Pinned in Task 3a.
3. The `source` channel is refused by the collector (`403`) and skipped by the crate (`uploads: false`, reason `source build`) independently. Pinned in Task 3a and Task 4.
4. `mode` never yields `uploads: true` from an empty `Settings`: the default is `Local`. `DO_NOT_TRACK=0` and `DO_NOT_TRACK=false` do not disable. `<APP>_TELEMETRY=log` forces `uploads: false`. Pinned in Task 4.
5. `seal` with a journal that has no mark line still produces a report (`focus` empty); a journal whose last line is cut mid-write is sealed from the complete lines; a `Lost` session's `len` is the last time the journal saw, not the sealing time. Pinned in Task 6.
6. `Queue::drain` with a transport that returns `Err` reschedules and never deletes; with `429` or `5xx` reschedules; with `400`, `403` or `413` deletes and counts `rejected`; a `session-*` file needs `usage == On` while a `crash-*` file needs only no environment reason. Pinned in Task 7.

---

### Task 1: The wire format, section 6, and its fixtures

**Files:**
- Modify: `spec/SPEC.md` (append section 6)
- Create: `spec/fixtures/app/check.json`, `session.json`, `crash.json`, `invalid.json`

**Interfaces:**
- Produces: the contract every later task tests against. `check.json` is `{ "query": "<query string>" }`. `invalid.json` is an array of `{ "reason": "<Invalid display text>", "status": 400|403, "report": {...} }` or `{ "reason", "status", "check": "<query string>" }`.

- [ ] **Step 1: Append section 6 to `spec/SPEC.md`**, transcribing the spec's 2.1 to 2.6 in the SPEC's register, with the Shared definitions above as the normative lists: transport (POST one object; GET check; `Content-Type` not checked; no `Origin`; the agent form), the envelope table, the `check`, `session` and `crash` tables with required and optional marked, the value rules, the decoration `{sv: 2, at}`, and the line (`report keys + s + time`, `time` the time of receipt with milliseconds).
- [ ] **Step 2: Write the three valid fixtures.** `check.json`: `v=2&app=arkos&ver=0.5.0&bin=daemon&os=macos&osv=15&arch=arm64&ch=release&k=check&rd=-1`. `session.json`: a full session with `focus` of three surfaces, four events (one with `d`, one without), `ev_dropped: 0`, `ev_rejected: 1`. `crash.json`: a panic with two frames, two images, `pk: "other"`, an `mh`, no `sig`, `mem: "unknown"`.
- [ ] **Step 3: Write `invalid.json`** with at least: unknown top-level key; top-level `s`; top-level `time`; unknown key in an event's `d`; `d` with 7 keys; a `d` value with a space; an event name with a capital; `ver` not a version; `v: 3`; `osv` a string; `os: "macOS"`; `ch: "source"` (403); `k: "view"`; a POST with `k: "check"`; `sid` of 8 hex chars; 201 events; session `rd: -2`; `frames` entry of three numbers; an `images` uuid of 31 chars; `up: "<2m"`; a check query missing `rd`; a check `rd=-2`; a check `rd=x`; a check with `k=session`; a check with `rd` twice.
- [ ] **Step 4: Commit** `docs(spec): section 6, app reports and checks, with fixtures`.

### Task 2: `visits-core::app`, the schema as code

**Files:**
- Create: `crates/visits-core/src/app.rs`
- Modify: `crates/visits-core/src/lib.rs` (`pub mod app; pub use app::{validate_report, validate_check, decorate_report, app_of, channel_of, kind_of, AppReport, AppDecoration};`), `crates/visits-core/src/validate.rs` (the four new `Invalid` variants and their `Display`)
- Create: `crates/visits-core/tests/app.rs`

**Interfaces:**
```rust
#[derive(Debug, Clone, PartialEq, Serialize)]   // no Deserialize: fields are built only by the validating walk
pub struct AppReport {
    #[serde(flatten)] pub fields: serde_json::Map<String, Value>,
    #[serde(skip_serializing_if = "Option::is_none")] pub s: Option<AppDecoration>,
    #[serde(skip_serializing_if = "Option::is_none")] pub time: Option<String>,
}
#[derive(Debug, Clone, PartialEq, Serialize)] pub struct AppDecoration { pub sv: u8, pub at: String }
pub fn validate_report(body: &[u8]) -> Result<AppReport, Invalid>;   // POST body: exactly one object; k=check is Invalid::Kind
pub fn validate_check(query: &str) -> Result<AppReport, Invalid>;     // GET query: the envelope plus rd; k must be check, else Invalid::Kind
pub fn decorate_report(r: AppReport, unix_millis: u64) -> AppReport;  // s = {2, rfc3339(secs)}, time = rfc3339_ms(millis)
pub fn app_of(r: &AppReport) -> &str;  pub fn channel_of(r: &AppReport) -> &str;  pub fn kind_of(r: &AppReport) -> &str;
```
Internals: `enum Kind { Str { cap: usize, pat: Pat }, Enum(&'static [&'static str]), Hex { min: usize, max: usize }, Int { min: i64 }, Bool, Focus, Events, Frames, Images }` and `enum Pat { Name, NameDash, Version, Value, Path, ImageName }` matched by hand (no `regex`). A `Field { key, kind, required }` table per section. The walk builds a fresh `Map` from the checked keys, so a client `s` or `time` is `Unknown`.

- [ ] **Step 1: Write `tests/app.rs` from the fixtures** (RED): every valid fixture validates and serializes back to the same JSON value; every entry of `invalid.json` fails with the entry's `reason` as `to_string()`; an array body fails with `Invalid::Json`; `decorate_report` sets `s.sv == 2`, `s.at` in whole seconds, `time` with millis; the serialized decorated report has exactly the fixture's keys plus `s` and `time`.
- [ ] **Step 2: Implement `app.rs`.** `ch == "source"` is checked last so a malformed source report is still `400`. Query parsing for the check: `serde_urlencoded` into pairs, a repeated key is `Pattern(key)`, then the same walk with the strings coerced by the field's kind.
- [ ] **Step 3: Run** `nice -n 15 cargo test -p visits-core -j 3` (GREEN), then the workspace.
- [ ] **Step 4: Commit** `feat(core): validate app reports and checks against section 6`.

### Task 3a: The report and check handlers

**Files:**
- Modify: `crates/visits/src/app.rs` (`AppState.apps: BTreeMap<String, Option<String>>`, `AppState.app_path: String`; `resolve_ip` and `limited` helpers; the `report` and `check` handlers; the routes mounted only when `apps` is not empty, the beacon route only when `origins` is not empty), `crates/visits/src/sink.rs` (`fn write_report(&self, r: &AppReport) -> io::Result<()>` on the trait with no default body; both impls)
- Modify: `crates/visits/tests/serve.rs` (`Capture` gains `reports: Mutex<Vec<AppReport>>`; `SharedSink` forwards `write_report`; a `start_apps(apps, limiter)` helper; `AppState` literals gain the two fields)
- Modify: `crates/visits/src/main.rs` (the `AppState` literal gains `apps: BTreeMap::new(), app_path: "/a".into()` so the workspace compiles; Task 3b replaces them with the flags)

**Interfaces:**
- `fn resolve_ip(state, headers, peer) -> Option<IpAddr>` (with the one-time proxy warning) and `fn limited(state, ip) -> Option<Response>` (the `429`), both extracted from `beacon` so that `beacon` keeps its own ignore answer and its behaviour byte for byte.
- `fn gate(headers, apps) -> Result<String, Response>`: an `Origin` header present is `403 "origin not allowed for reports"`; `User-Agent` must be `<app>/<rest>` with `<app>` in `apps` (else `403 "app not allowed"`); returns the app.
- `report`: `POST` only (`405`, `Allow: POST`); `resolve_ip`; an ignored address answers `204` unwritten; `limited`; `gate`; `validate_report`; `app_of == gate's app` (else `403 "app not allowed"`); `channel_of == source` is `403 "source channel"`; `decorate_report`; `sink.write_report`; `204`. The route carries its own `DefaultBodyLimit::max(64 * 1024)` on the method router (`.route(&app_path, any(report).layer(DefaultBodyLimit::max(64 * 1024)))`), inside the router's 16 KB layer, so the inner wins.
- `check`: `GET` only; `resolve_ip`; `limited`; `gate`; `validate_check`; app match; source `403`; decorate and write unless the address is ignored (an ignored check is still redirected, unwritten); then `302` with `Location: <redirect>` or `204` when the app has none.
- No `--log-ip` print on either route.

- [ ] **Step 1: Tests in `serve.rs`** (RED), with `start_apps` using `Limiter::new(1000.0, 0.0)`: the session fixture posted with `User-Agent: arkos/0.5.0`, `X-Forwarded-For: 203.0.113.9` and that UA is written with `s.sv == 2`, and the captured report serialized as the sink does contains neither `203.0.113.9`, the UA, `visit`, `host`, `cc` nor `bn`; the check query answers `302` with the configured `Location` and is written first; a check for an app without a redirect answers `204`; an ignored address's check is `302` and unwritten; an `Origin` header on `/a` and on `/a/check` is `403`; an unknown app, a bare agent, and an agent naming another app than the body are `403`; `ch=source` is `403` and not written; every entry of `invalid.json` (each has `app: "arkos"`, sent with `User-Agent: arkos/0.5.0`, so the gate passes and validation decides) posted, or fetched for checks, answers its `status` and writes nothing; an array body is `400`; a 17 KB report is `204`; a 65 KB body is `413`; `GET` on `/a` and `POST` on `/a/check` are `405`; with burst 3 and `Limiter::new(3.0, 0.0)`, two beacons and two reports from one address make the fourth `429` (one bucket); with `apps` empty `/a` is `404` and every existing beacon test passes unchanged.
- [ ] **Step 2: Implement.**
- [ ] **Step 3: Run** `nice -n 15 cargo test -p visits -j 3` (GREEN), the workspace, and `nice -n 15 cargo clippy --workspace --all-targets -j 3` with no new warning.
- [ ] **Step 4: Commit** `feat(collector): the report and check routes for installed programs`.

### Task 3b: The flags

**Files:**
- Modify: `crates/visits/src/config.rs` (`--app NAME[=REDIRECT]`, repeatable, env `VISITS_APPS` comma-separated; `--app-path`, env `VISITS_APP_PATH`, default `/a`; `--origin` no longer `required`, its test literal updated), `crates/visits/src/main.rs` (parse `NAME[=REDIRECT]`; `NAME` must match the name-with-dash pattern; `REDIRECT` must start with `https://`; at least one of `--origin` or `--app` must be given; `--app-path` must start with `/`, differ from `--path` and `/healthcheck`, and `--path` must not equal `--app-path` or `--app-path/check`; print the apps at start)
- Modify: `crates/visits/src/config.rs` tests (`parse_app("arkos=https://x")`, `parse_app("arkos")`, a bad name, an `http://` redirect)

- [ ] **Step 1: Tests** (RED) for the parser and the path checks as pure functions in `config.rs`.
- [ ] **Step 2: Implement.**
- [ ] **Step 3: Run** the crate's tests; **commit** `feat(collector): --app and --app-path`.

### Task 4: `visits-app`: the crate, the types and the mode

**Files:**
- Create: `crates/visits-app/Cargo.toml` (`visits-core`, `serde`, `serde_json`, `toml`), `crates/visits-app/src/lib.rs`, `src/report.rs`, `src/mode.rs`
- Create: `crates/visits-app/tests/report.rs`, `tests/mode.rs`

**Interfaces:**
```rust
pub enum Os { Macos, Linux, Windows }      pub enum Arch { Arm64, X86_64 }
pub enum Channel { Release, Source, Named(String) }   // Named("tap"): validated by the name-with-dash pattern at to_bytes
pub struct Envelope { pub app: String, pub ver: String, pub bin: String, pub os: Os, pub osv: u32, pub arch: Arch, pub ch: Channel }
pub enum DValue { Str(String), Int(i64), Bool(bool) }
pub struct Event { pub n: String, pub t: u64, pub d: BTreeMap<String, DValue> }
pub enum End { Quit, Lost, Unknown }
pub struct Session { pub env: Envelope, pub sid: String, pub len: u64, pub end: End, pub iw: i64, pub rd: i64, pub focus: BTreeMap<String, u64>, pub ev: Vec<Event>, pub ev_dropped: u64, pub ev_rejected: u64 }
impl Session { pub fn push(&mut self, ev: Event); }   // keeps the first 200, counts the rest in ev_dropped
pub enum CrashKind { Panic, Signal }   pub enum Uptime { Lt1m, Lt10m, Lt1h, Lt1d, Ge1d }   pub enum Memory { Normal, Warn, Critical, Unknown }
pub struct Crash { pub env: Envelope, pub inc: String, pub ck: CrashKind, pub commit: String, pub pk: String, pub mh: Option<String>, pub file: String, pub line: u32, pub frames: Vec<(u32, u64)>, pub images: Vec<(String, String, u64)>, pub sig: Option<i32>, pub up: Uptime, pub mem: Option<Memory> }
pub struct Check { pub env: Envelope, pub rd: i64 }
impl Session / Crash { pub fn to_bytes(&self) -> Result<Vec<u8>, Invalid>; }   // a Map built by hand (v: 2, k, lowercase enums, channel as its string, empty d and None omitted), then validate_report; the exact wire bytes
impl Check { pub fn to_query(&self) -> Result<String, Invalid>; }              // the GET query in the envelope's key order, then validate_check

pub enum Usage { Off, Local, On }    pub enum Crashes { Never, Ask, Always }   // every enum in this crate derives Clone, Copy (where fieldless), PartialEq, Eq, Debug
#[derive(Default)] pub struct Settings { pub usage: Option<Usage>, pub crashes: Option<Crashes> }
#[derive(Clone, Debug, PartialEq)] pub struct Mode { pub usage: Usage, pub crashes: Crashes, pub uploads: bool, pub invite: bool, pub log: bool, pub reason: Option<&'static str> }
pub fn mode(settings: &Settings, env: &dyn Fn(&str) -> Option<String>, app_env_var: &str, channel: &Channel, interactive: bool) -> Mode;
```
The mode table is the spec's 4.2 in order, with the reasons from Shared definitions; `log` forces `uploads: false` with reason `"telemetry log"`; `invite` is true only with no reason, `usage == Local` and a non-source channel. Deviation from spec 4.1, noted in the crate docs: public fields plus `to_bytes` validation instead of refusing constructors; the wire check is the same.

- [ ] **Step 1: Tests** (RED): `to_bytes` of the session and crash fixtures' values equals the fixtures' JSON values; `to_query` of the check fixture's values equals its query; `push` past 200 sets `ev_dropped`; a `d` value with a space fails with `Invalid::Pattern("ev.d.x")`; `Channel::Named("Tap")` fails `Pattern("ch")`; the mode table, one test per rule, including `DO_NOT_TRACK=0`, `=false`, `=1`, `=anything`; `=log` with settings `On` gives `uploads: false, log: true`; `CI` set keeps `Local` recording with `uploads: false` and reason `"CI"`; a source channel gives `uploads: false, invite: false`; an empty `Settings` gives `Local`, `Ask`, `uploads: false`, `invite: true` when interactive and release.
- [ ] **Step 2: Implement.**
- [ ] **Step 3: Run** `nice -n 15 cargo test -p visits-app -j 3` (GREEN); the workspace.
- [ ] **Step 4: Commit** `feat(app): the visits-app crate, report types and the consent mode`.

### Task 5: The allowlist and the journal

**Files:**
- Create: `crates/visits-app/src/allowlist.rs`, `src/journal.rs`, `tests/journal.rs`
- Modify: `src/lib.rs`

**Interfaces:**
```rust
#[derive(Debug, PartialEq)] pub enum Rejected { Name(String), Key(String, String), Value(String, String) }   // event; event, key; event, key
pub struct Allowlist { .. }
impl Allowlist {
    pub fn from_toml(text: &str) -> Result<Allowlist, String>;   // [events.<name>] tables; each key = ["a", "b"] | "int" | "bool"; refuses what the wire would: a bad name, more than 6 keys, a value failing the value pattern or cap
    pub fn check(&self, ev: &Event) -> Result<(), Rejected>;
}
pub struct Journal { .. }
impl Journal {
    pub fn open(dir: &Path, sid: &str, env: &Envelope, allow: Allowlist, now: u64) -> io::Result<Journal>;  // sid must be 16 to 32 hex (else InvalidInput); creates dir/journal/<sid>.jsonl with the start line {"start": now, "env": {...}}
    pub fn event(&mut self, ev: Event, now: u64);   // t is set from now - start; checks; appends {"ev": {..}} or counts rejected
    pub fn focus(&mut self, surface: &str, secs: u64);
    pub fn mark(&mut self, now: u64) -> io::Result<()>;   // appends {"mark": {"t": now - start, "focus": {..}, "rejected": n}}
}
```
The allowlist file's shape, documented in `lib.rs`:
```toml
[events.pane_open]
kind = ["terminal", "viewer", "browser"]
how = ["palette", "key", "click"]
[events.window_open]
```

- [ ] **Step 1: Tests** (RED): a TOML with two events parses; a bad name, a seventh key and a value with a space are refused by `from_toml`; a name, a key and a value not in it are each `Rejected` with the right variant; `"int"` admits integers only; `open` with `sid` `../x` is an error; the file has the start line with the envelope, one line per accepted event with `t`, and a mark line with the focus totals, `t` and the rejected count; a second `mark` appends a second line.
- [ ] **Step 2: Implement.** Lines are written with one `write_all` of the whole line (`serde_json::to_vec` plus `\n`) so a crash leaves at most one cut line at the end.
- [ ] **Step 3: Run** and **commit** `feat(app): the allowlist and the journal`.

### Task 6: State and sealing

**Files:**
- Create: `crates/visits-app/src/state.rs`, `src/seal.rs`, `tests/seal.rs`

**Interfaces:**
```rust
#[derive(Default, Clone, Debug, PartialEq)] pub struct State { pub first_launch_day: Option<i64>, pub last_report_day: Option<i64>, pub last_check_day: Option<i64>, pub rejected: u64, pub sent: u64 }
impl State { pub fn load(dir: &Path) -> State; pub fn save(&self, dir: &Path) -> io::Result<()>; }   // dir/state.toml, written through a temp file and rename
pub fn seal(dir: &Path, sid: &str, end: End, now: u64, state: &mut State) -> io::Result<Option<PathBuf>>;
pub fn seal_stale(dir: &Path, now: u64, older_than_secs: u64, state: &mut State) -> io::Result<Vec<PathBuf>>;
```
`seal` reads the journal's complete lines (a last line without `\n` is ignored), takes the start line's time and envelope (so a session sealed after an upgrade keeps the version that ran), the events in order, the last mark's focus and rejected count; `len` is `now - start` for `Quit` and the greatest `t` seen (mark or event) for `Lost` and `Unknown`; `iw` and `rd` come from the state (`first_launch_day` set at the first seal; `rd = -1` without a `last_report_day`); writes `dir/queue/session-<sid>.json` as `Session::to_bytes` with its `.retry` companion (`first: now, attempts: 0, next: now`) (a session that fails validation is dropped, `state.rejected += 1`, never queued); updates the state's days; deletes the journal; returns the path. `seal_stale` seals, as `Unknown`, every journal whose greatest `t` plus its start is older than `now - older_than_secs`.

- [ ] **Step 1: Tests** (RED): a journal with four events and two marks seals to a report with the second mark's focus and the envelope from the start line; no mark seals with empty focus; a cut last line is ignored; `Lost` uses the last `t`; `rd` is `-1` the first time and the day difference the second; `iw` is a multiple of 7 and set once; `seal_stale` seals the old journal and leaves the fresh one.
- [ ] **Step 2: Implement.**
- [ ] **Step 3: Run** and **commit** `feat(app): state and sealing a journal into a session report`.

### Task 7: The queue and the transport

**Files:**
- Create: `crates/visits-app/src/queue.rs`, `tests/queue.rs`

**Interfaces:**
```rust
pub trait Transport { fn post(&self, url: &str, user_agent: &str, body: &[u8]) -> Result<u16, String>; }
pub struct Queue { dir: PathBuf }
#[derive(Default, Debug, PartialEq)] pub struct Outcome { pub sent: usize, pub failed: usize, pub rejected: usize, pub dropped: usize, pub skipped: usize }
impl Queue {
    pub fn new(dir: &Path) -> Queue;
    pub fn enqueue_crash(&self, crash: &Crash) -> io::Result<PathBuf>;     // inc must be 16 to 32 hex; queue/crash-<inc>.json
    pub fn due(&self, now: u64) -> Vec<PathBuf>;                           // by the .retry companion {"first": secs, "attempts": n, "next": secs}; a file with none is due now
    pub fn drain(&self, t: &dyn Transport, url: &str, user_agent: &str, mode: &Mode, now: u64, state: &mut State) -> Outcome;
}
```
With `mode.log`, every due file is printed to stderr as `visits-app: would send <path>: <bytes>` and kept, and the transport is never called. Otherwise, per file: a `session-*` file is sent only when `mode.usage == On && mode.uploads`; a `crash-*` file only when `mode.uploads` (queuing it was the consent; `uploads` is false under any environment reason). A file not eligible is `skipped` and kept. Outcomes of a post: `2xx` deletes both files and bumps `state.sent`; `429`, `5xx` and `Err` reschedule (15 min, 1 h, 4 h, 24 h, then daily); other `4xx` delete and bump `state.rejected`. `seal` and `enqueue_crash` write the `.retry` companion with `first: now, attempts: 0, next: now` when they create a file, so no file time is ever read; any file whose `first` is older than seven days is deleted and counted `dropped`, attempts or not, so the queue never grows without bound under `Local`.

- [ ] **Step 1: Tests** (RED): a fake transport recording calls and answering a scripted status; the outcomes of Review Focus 6; the backoff times; the drop after seven days with and without attempts; `drain` with `uploads: false` calls the transport zero times and reports `skipped`; `usage: Local` with a crash file sends the crash and skips the session; the `Mode` that `mode()` returns for `<APP>_TELEMETRY=log` with settings `On` prints every due file, keeps them, and calls the transport zero times.
- [ ] **Step 2: Implement.**
- [ ] **Step 3: Run** and **commit** `feat(app): the queue, its backoff and the transport trait`.

### Task 8: The check and the view

**Files:**
- Create: `crates/visits-app/src/check.rs`, `src/view.rs`, `tests/check.rs`
- Modify: `src/lib.rs` (the public surface, crate docs with the program's loop in ten lines)

**Interfaces:**
```rust
impl Check { pub fn next(env: &Envelope, state: &State, now: u64) -> Option<Check>; }   // Some once per UTC day; rd from last_check_day, -1 without one; None for Channel::Source
impl State { pub fn checked(&mut self, now: u64); }                                       // after the request, whatever it answered
pub struct View { pub mode: Mode, pub state: State, pub queued: Vec<(PathBuf, u32, u64)>, pub last_session: Option<PathBuf>, pub journals: Vec<PathBuf> }
pub fn view(dir: &Path, mode: Mode, now: u64) -> View;
pub fn payload_of(path: &Path) -> io::Result<Vec<u8>>;
```

- [ ] **Step 1: Tests** (RED): `next` is `Some(rd: -1)` with no state, `None` the same day after `checked`, `Some(rd: 1)` the next day, `Some(rd: 3)` three days on, and across a UTC midnight by one second; `None` for a source build whatever the state; `view` lists the queue with attempts and next times and the newest session file.
- [ ] **Step 2: Implement.** `lib.rs` documents the loop: open the journal at launch, `mark` each minute, `seal` when the process ends, `drain` every 15 minutes, `Check::next` before an update fetch, save the state after each.
- [ ] **Step 3: Run** `nice -n 15 cargo test --workspace -j 3` and clippy; **commit** `feat(app): the daily check and the view`.

### Task 9: The README, AGENTS.md and the cloud paragraph

**Files:**
- Modify: `README.md` (an "Installed programs" section after "More": the two routes, `--app NAME[=REDIRECT]`, the crate and the allowlist file, what a report may and may not carry, the `source` rule; one paragraph under "Running it in the cloud" about the platform's request log and the exclusion), `AGENTS.md` (the app route, the crate, section 6 and its fixtures)

- [ ] **Step 1: Write the sections** in the README's register.
- [ ] **Step 2: Commit** `docs: installed programs, the report route, the crate and the request-log caveat`.

### Task 10: Version 0.3.0

**Files:**
- Modify: `Cargo.toml` (workspace version `0.3.0`), `Cargo.lock`

- [ ] **Step 1: Bump**, run the workspace tests once more, **commit** `chore: v0.3.0`, tag `v0.3.0` (the release workflow builds the collector from the tag; the crate is consumed by git tag).
