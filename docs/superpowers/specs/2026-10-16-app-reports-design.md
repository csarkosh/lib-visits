# App reports: lib-visits as the collector for installed software

**Date:** 2026-10-16
**Status:** Draft for review
**Depends on:** the wire specification version 1 (`spec/SPEC.md`), the collector (`crates/visits`), the pure half (`crates/visits-core`)
**First consumer:** Arkos IDE (`csarkosh/ide-arkos`, its own design in that repository, `docs/superpowers/specs/2026-10-16-telemetry-design.md`)
**Research:** csarko.sh `docs/published/2026-10-15-telemetry-without-identifying-anyone.md`

## 1. What this adds

lib-visits counts visits to a website from a script in the page. This adds the other kind of client: a program installed on someone's computer that wants to tell its maintainer how it is used and when it breaks, without carrying anything that identifies the computer or its owner. The collector gains one route for typed reports and one for update checks; a new crate gives the program the pure half of being a good client: the report types, the journal it writes while it runs, the queue it uploads from, and the consent rules it has to obey. The program keeps the network, the user interface and the decision of what to record.

The design keeps the research's conclusions as rules that the collector enforces, not conventions the client is asked to follow:

- **No identifier, ever.** A report has no machine id, install id, user id or cookie. The one random value in it, the session id, is made when a session starts and dies with it; nothing ties two reports to one computer.
- **No address, ever.** The collector computes no visit hash for a report and writes nothing about the request's address. The address exists in memory for the rate limiter and nowhere else.
- **Typed, allowlisted, capped.** Every field has a type, a cap and a reason. A report with a key the schema does not know is refused whole, not trimmed, so a client bug cannot leak through a lenient server.
- **Consent is the client's job, and the crate makes it hard to get wrong.** The crate decides the effective mode from the environment and the program's settings, and the mode that uploads is never the default.

### What it will not do

- Identify a client across two reports, or let the maintainer do so by hand. There is no field that could.
- Accept free text. Every string is an enumerated value, a version, a path remapped at build time, or a hash.
- Serve the update itself. The check route answers with a redirect to wherever the program's updates live; the bytes never pass through the collector.
- Replace the web client. Pages keep the beacon route; the report route is a second door with its own validation.

## 2. The wire format, version 2 (an addition to `spec/SPEC.md`)

Version 1 stays as it is. Section 6 of the wire specification describes reports and checks.

### 2.1 Transport

- `POST <report path>` (default `/a`): the body is the JSON of one report. `Content-Type: application/json`. Body cap 64 KB. One report per request; arrays are refused (`400`).
- `GET <report path>/check`: the check's keys as query parameters, nothing else. Answers `302` with `Location` set to the app's configured redirect, or `204` when the app has none. Either way `cache-control: no-store`.
- `User-Agent` must be `<app>/<version>` optionally followed by a space and anything, where `<app>` is the report's `app` field (`403` otherwise). There is no `Origin`; a request that carries one is refused (`403`), since a page must not post reports.
- The app named must be allowlisted at the collector (`--app`), `403` otherwise.
- Statuses: `204` accepted (POST) or no redirect (GET), `302` check redirected, `400` malformed or unknown key, `403` app, agent or origin not allowed, `413` too large, `429` rate limited, `405` other methods. The body of an error is `{"error": "<reason>"}` as for beacons.

The rate limiter is the beacon route's, by address, in memory, shared between the two routes.

### 2.2 The envelope

Every report and every check carries these keys, and a report carries nothing else outside its kind's section.

| Key | Type | Cap | Meaning |
|---|---|---|---|
| `v` | integer | | `2` |
| `app` | string | 32 | the program's name, `^[a-z][a-z0-9_-]{0,31}$`, allowlisted at the collector |
| `ver` | string | 32 | the program's version, `^[0-9]+\.[0-9]+\.[0-9]+([-+][0-9A-Za-z.-]+)?$` |
| `bin` | string | 32 | which binary of the program, `^[a-z][a-z0-9_-]{0,31}$`, e.g. `app`, `daemon` |
| `os` | string | 16 | `macos`, `linux`, `windows` |
| `osv` | integer | | the OS major version only, e.g. `15` |
| `arch` | string | 16 | `arm64` or `x86_64` |
| `ch` | string | 16 | the channel the program was installed from: `release` or `source`; a program may add its own names (`tap`, `direct`) |
| `k` | string | 16 | the kind: `check`, `session`, `crash` |

The collector refuses a report whose `ch` is `source` (`403`); the crate never uploads one either. The double guard is deliberate: a source build that has been patched to upload still counts for nothing.

### 2.3 `check`

The check is what a self-updating program sends when it asks for its update manifest, and it is the one thing sent without consent, because the person installed a self-updating tool. It carries the envelope and one more key:

| Key | Type | Meaning |
|---|---|---|
| `rd` | integer | days since this program last sent a check through this route, rounded down; `-1` on the first ever |

A client sends at most one check a day through the collector and goes straight to its update source the rest of the day, so one row is one machine-day. `rd` makes the row say whether the machine is new (`-1`), was here yesterday (`1`), or is back after a week (`7`), which is all an active count and a retention curve need, with nothing stored about the machine on the server. The client keeps the date of its last check in a file beside its other state; the research's Omaha reference calls this `rd`.

### 2.4 `session`

One session of the program, from launch to exit, sent after it ends.

| Key | Type | Cap | Meaning |
|---|---|---|---|
| `sid` | string | 32 | the session's random id, `^[0-9a-f]{16,32}$`, made at launch and never written anywhere but the journal |
| `len` | integer | | the session's length in seconds |
| `end` | string | 16 | `quit`, `lost` (the process went away without saying goodbye), `unknown` |
| `iw` | integer | | the install week: days since the epoch of the first launch, rounded down to a multiple of 7 |
| `rd` | integer | | days since the last session report, `-1` on the first |
| `focus` | object | 16 keys | seconds of foreground time per named surface, key `^[a-z][a-z0-9_]{0,31}$`, value integer |
| `ev` | array | 200 | the events, in order |

An event is an object of `n` (name, `^[a-z][a-z0-9_]{0,31}$`), `t` (integer seconds since launch) and optionally `d` (an object of at most 6 keys, each `^[a-z][a-z0-9_]{0,31}$`, whose values are strings of at most 64 characters matching `^[A-Za-z0-9_.:/-]*$`, integers, or booleans). Past 200 events the client keeps the first 200 and sets `ev_dropped` (integer) to the number lost.

The value pattern admits a version, an enum name, a command id or a remapped path and refuses a sentence, a file name with spaces, a prompt or a URL with a query. The program's own allowlist, which the crate enforces before writing the journal, is what says which names and which values exist; the collector enforces only the shape. The program publishes that allowlist as its schema.

### 2.5 `crash`

One incident, sent only after the person chose to send it.

| Key | Type | Cap | Meaning |
|---|---|---|---|
| `inc` | string | 32 | the incident id, random, `^[0-9a-f]{16,32}$`; the receipt the person can quote |
| `ck` | string | 16 | `panic`, `signal` |
| `commit` | string | 40 | the build's git hash |
| `pk` | string | 64 | the panic kind, an enum from the program's own code paths, or `other` |
| `mh` | string | 64 | SHA-256 of the panic message's first line, lowercase hex, when `pk` is `other` |
| `file` | string | 256 | the panic's source file as the build remapped it (`/src/...`), `^[A-Za-z0-9_./-]*$` |
| `line` | integer | | the panic's line |
| `frames` | array | 64 | the faulting thread's frames, each `[image index, offset]` of integers |
| `images` | array | 32 | the images referenced, each `[name, uuid, size]`: `^[A-Za-z0-9_.-]{1,64}$`, 32 lowercase hex, integer |
| `sig` | integer | | the signal number, for `signal` |
| `up` | string | 16 | the process's uptime bucket: `<1m`, `<10m`, `<1h`, `<1d`, `>=1d` |
| `mem` | string | 16 | memory pressure at the time if known: `normal`, `warn`, `critical`, `unknown` |

The message itself never travels. A panic the program raised on purpose has a kind; one it did not has a hash, which groups incidents without saying what they said. The frames are offsets into named images; symbol files stay with the maintainer.

### 2.6 The decoration

Beside the client's keys the collector adds `s`:

| Key | Value |
|---|---|
| `sv` | `2` |
| `at` | RFC 3339 UTC, whole seconds |

No `visit`, no `host`, no browser, no OS from headers (the report says its own), no country. The JSON line is the report's keys, `s`, and `time` as for beacons (the time of receipt; reports carry no clock of their own, so there is nothing to prefer).

## 3. The collector

### 3.1 Configuration

- `--app NAME[=REDIRECT]` (repeatable; env `VISITS_APPS`, comma-separated): a program allowed to post, and where its checks are redirected. Without any `--app` the report route is not mounted and the collector behaves exactly as today.
- `--app-path` (env `VISITS_APP_PATH`, default `/a`): the report route; the check route is that plus `/check`.
- `REDIRECT` must be an `https://` URL; the collector refuses to start otherwise.

The beacon route's `--origin` requirement stays; a collector that serves only apps still has to name one origin today, which is a wart to fix in the same change by making `--origin` required only when `--app` is absent.

### 3.2 The request

`crates/visits/src/app.rs` gains two handlers beside `beacon`: `report` and `check`. Both run the limiter first (the same bucket as beacons), then the agent and app gates, then `visits_core::app::validate_report` or `validate_check`, then the sink. Validation is in `visits-core` so the client crate validates with the same code before anything is written; a report that the client wrote cannot be one the collector refuses, except by a version skew the client crate's own version pins.

### 3.3 Logging hygiene, stated for the operator

The collector writes no address. The host may: Cloud Run's request log carries the remote address for every request. The README's cloud section gains one paragraph: exclude the service's request-log entries from the default bucket, or accept that the platform keeps addresses for its default retention, and say which on the program's disclosure page. csarko.sh's Terraform already excludes successful beacon requests; it has to cover `/a` and the check redirects (status `302`) too.

## 4. The client crate `visits-app`

`crates/visits-app`, GPL-3.0-only like the rest, pure: no network, no clock of its own beyond `SystemTime`, no threads. The program supplies the transport and the user interface.

### 4.1 Types and validation

`Envelope`, `Check`, `Session`, `Event`, `Crash`, with serde, built by constructors that take the enums and refuse the rest; `to_bytes()` validates with `visits_core::app` and returns the exact bytes that go on the wire, which is also what the program shows the person.

### 4.2 Mode

```
pub enum Usage { Off, Local, On }
pub enum Crashes { Never, Ask, Always }
pub struct Mode { usage: Usage, crashes: Crashes, uploads: bool, reason: Option<&'static str> }
pub fn mode(settings: &Settings, env: &dyn Fn(&str) -> Option<String>, channel: Channel, interactive: bool) -> Mode
```

The rules, in order, first match wins for `uploads: false` with its `reason`:

1. `channel == Source`: never uploads, never shows an invitation. Reason `source build`.
2. `DO_NOT_TRACK` set to anything but empty, `0` or `false`: usage `Off`, crashes `Never`. Reason `DO_NOT_TRACK`.
3. `<APP>_TELEMETRY=off`: the same. `=log`: the mode the settings say, with `log: true`, so the program prints every payload to stderr instead of sending it.
4. `CI` set, or not interactive: uploads off, no invitation, local recording continues. Reason `CI` or `not interactive`.
5. Otherwise the settings: `usage` is `Local` unless the settings say `On` or `Off`; `crashes` is `Ask` unless they say otherwise.

The default is `Local`: the journal and the queue are written on the person's disk and nothing leaves until they say so. `Off` stops the writing too, as Go's does.

### 4.3 The journal

While the program runs, `Journal::open(dir, sid)` appends one line per event to `<dir>/journal/<sid>.jsonl` with the program's allowlist checked first (`Allowlist::from_toml`, the program's `telemetry/events.toml`: names, their permitted `d` keys and, per key, the permitted values or `int` or `bool`). `Journal::focus(surface, secs)` accumulates foreground seconds in memory and the program flushes them with `Journal::mark()` at most once a minute, as a line that replaces the previous totals. An event that fails the allowlist is dropped and counted; the count is reported as `ev_rejected` in the session report, so a program bug shows up in the data instead of in someone's file.

### 4.4 Sealing

`seal(dir, sid, end: End, now) -> Option<PathBuf>` turns a journal into a session report in `<dir>/queue/session-<sid>.json`, computing `len`, `iw`, `rd` from `<dir>/state.toml` (the first launch date and the last report date, which it updates), and deletes the journal. The program's resident part calls it when the session ends: on a clean exit with `Quit`, or when the resident part notices the process is gone with `Lost`. A journal older than a day with no process behind it is sealed `Unknown` at the next launch.

### 4.5 The queue and the uploader

`Queue::new(dir)` lists `queue/*.json`. `Queue::due(now)` yields the reports whose retry time has come: the first attempt at once, then 15 minutes, an hour, four hours, a day, then dropped after seven days, with the attempt count and next time in the file's companion `.retry`. The program's loop calls `queue.due(now)`, posts each with its own transport, and tells the queue `sent(path)` (deleted) or `failed(path, now)` (rescheduled). A `4xx` other than `429` is `rejected(path)`: deleted, counted in `state.toml` as `rejected`, visible in `view`, since retrying a report the collector refuses is pointless. Crash reports are queued by the program only once the person chose Send, or at once when crashes are `Always`.

```
pub trait Transport { fn post(&self, url: &str, user_agent: &str, body: &[u8]) -> Result<u16, String>; }
```

The program's transport sends with a short timeout and never blocks a user action; the crate never calls it from anything but the loop the program runs.

### 4.6 The check

`Check::next(state, today) -> Option<Check>`: `Some` once a UTC day, with `rd` computed from the state's last check date, which the program updates after the request was made (whatever it answered). The program routes that one fetch through the collector and the day's other fetches straight to its update source.

### 4.7 View

`view(dir) -> View`: the mode, the counts, the queued reports and their next attempts, the last sealed session, and `payload_of(path)` for the bytes of any one, so the program's `telemetry view` command and its "Show report" button print the truth from disk.

## 5. Testing

- `visits-core`: the fixtures in `spec/fixtures/app/*.json` (valid and invalid reports and checks, each with the expected status or the refused key), read by the collector's tests and the crate's.
- `visits`: route tests for the agent, origin and app gates, the body cap, the check redirect and the `204`, the `source` refusal, and that a report's JSON line carries no address and no visit.
- `visits-app`: the mode table as a test per rule; the journal's allowlist refusals and the rejected count; sealing from a journal with and without a mark line; the retry schedule and the drop after seven days; `Check::next` across a day boundary and on the first run.

## 6. Build and release

The crate is published as a git dependency pinned by tag (`visits-app = { git = "https://github.com/csarkosh/lib-visits", tag = "v0.3.0" }`); crates.io later if a second program wants it. The collector's version becomes 0.3.0 with the route; csarko.sh deploys it with `--app arkos=https://github.com/csarkosh/ide-arkos/releases/latest/download/manifest.json` and a Hosting rewrite of `apps.csarko.sh/a` and `/a/check` to the existing `csarko-visits` service, so one service and one bucket hold both kinds of rows, told apart by `app`.

## 7. Out of scope

- Aggregation and dashboards: rows land in the linked BigQuery dataset; what reads them is the program's business and comes last.
- A persistent freshness token for clone detection (the research's `report_freshness`): not until a clone has been seen in the data.
- Signal crashes and minidumps: the crash kind exists in the schema; the first consumer sends panics only.
- Country from the address: never for reports.
