# lib-visits wire specification, version 1

## 1. Transport

A client sends one event per request, or an array of up to 50, to the collector's path (default `/v`).

- `POST`: the body is the JSON of one event, or a JSON array of at most 50 events.
  `navigator.sendBeacon(path, string)` sends it as `text/plain`, so no preflight.
- `GET`: the event's keys as query parameters. Numbers and booleans are their
  decimal and `true`/`false` spellings; `d` is JSON-encoded as one parameter.

Caps: body 16 KB. The origin is the `Origin` header, else the allowlisted origin that `Referer` starts with,
and must be allowlisted. Statuses: `204` accepted, `400` malformed, `403` origin
not allowed, `413` too large or more than 50 events, `429` rate limited, `405`
other methods. Every answer carries `cache-control: no-store`. The body of an
error is `{"error": "<reason>"}`.

The limiter is a courtesy filter, not a boundary: the route is public. The client address is only as trustworthy as `--trusted-proxies` is accurate (a value higher than the real proxy count lets a client choose its own address), and when the limiter's table is full of fresh addresses, newcomers are served untracked rather than real visitors refused. A `--trusted-proxies` wrong in either direction, too high or too low, resolves every visitor to one address or to none, so they share one bucket and no visit is computed. A host under a flood answers it at its reverse proxy or CDN.

## 2. Events

Every event: `v` (integer, `1`), `n` (name), `id` (1 to 32 chars, the page load's
random id), `p` (path, ≤ 2048 chars). Unknown keys are dropped. A key of the
wrong type fails the request. An array with one invalid event fails whole.

Names: `view`, `leave`, `click`, or a custom name matching `^[a-z][a-z0-9_-]{0,31}$`
(`vitals` is one). A custom event's payload is `d`: an object of at most 10 keys
whose values are strings (≤ 200 chars), numbers or booleans.

| Key | Type | Cap | Meaning |
|---|---|---|---|
| `t` | string | 200 | title |
| `r` | string | 2048 | referrer, origin and path only: no query, no fragment |
| `us` `um` `uc` `ut` `uo` `cr` | string | 100 | utm_source, utm_medium, utm_campaign, utm_term, utm_content, ref |
| `nt` | string | 16 | `navigate`, `reload`, `back_forward`, `prerender` |
| `sw` `sh` `vw` `vh` | integer | | screen and viewport size |
| `dpr` | number | | device pixel ratio |
| `th` | string | 8 | `dark` or `light` |
| `rm` `tp` `re` | boolean | | reduced motion, touch, reached the end |
| `tz` | string | 64 | IANA timezone |
| `lg` | string | 32 | language tag |
| `b` | integer | | bot marker: 1 webdriver (the client sends only this; 2 PhantomJS, 3 Nightmare, 4 Selenium are accepted from older clients) |
| `e` | integer | | engaged seconds |
| `sd` | integer | | max scroll depth 0 to 100 |
| `ev` | string | 64 | the named event of a click |
| `u` | string | 2048 | origin plus path of a click to another host; with `ev` when that link is also named |
| `ts` | integer | | the client's clock at the event, milliseconds since the epoch (section 5) |
| `d` | object | 10 keys | a custom event's payload |

## 3. The decoration `s`

Added by the collector beside the client's keys, never over them.

| Key | Value |
|---|---|
| `sv` | `1` |
| `at` | RFC 3339 UTC time of the request, whole seconds, e.g. `2026-10-15T02:40:30Z` |
| `visit` | 16 lowercase hex chars (section 4); absent when no secret is configured |
| `host` | the resolved origin's host |
| `bn` `bv` | browser family and major version (integer), or `other` and absent |
| `os` | `Windows`, `macOS`, `iOS`, `Android`, `Linux`, `ChromeOS`, `other`; a platform hint outside this set is `other` |
| `bot` | `true` when `b` is non-zero or the User-Agent contains (case-insensitive) one of: bot, crawl, spider, slurp, headless, lighthouse, pagespeed, gtmetrix, facebookexternalhit, preview |
| `rk` | `direct` (no referrer), `internal` (referrer host equals `host` or is a subdomain of it), `external` |
| `rp` | internal only: the referrer's path |
| `rh` | external only: the referrer's host, leading `www.` removed; absent when the referrer has no parseable `scheme://host` |
| `lg` | first tag of `Accept-Language`, only when the event has no `lg` and the tag is 1 to 35 characters of letters, digits and hyphens |
| `cc` | ISO 3166-1 alpha-2 country, when a GeoIP database is configured |

Browser from `Sec-CH-UA` when present: the brand that is neither `Chromium` nor
starts with `Not`, else `Chromium`; its `v`; a hint brand over 32 characters is ignored and the User-Agent decides. `Google Chrome` is reported as `Chrome` and `Microsoft Edge` as `Edge`, so the name is the same with or without hints. Else from the User-Agent, first
match in this order: `Edg/` Edge, `OPR/` Opera, `SamsungBrowser/` Samsung Internet,
`CriOS/` Chrome, `FxiOS/` Firefox, `Firefox/` Firefox, `Chrome/` Chrome,
`Version/` with `Safari/` Safari; the major version is the integer after the token.
OS from `Sec-CH-UA-Platform` when present (`Chrome OS` becomes `ChromeOS`), else
from the User-Agent: `Windows`, `Android` (before Linux), `iPhone`/`iPad`/`iPod`
iOS, `Mac OS X` macOS, `CrOS` ChromeOS, `Linux`.

Host comparison strips a leading `www.` and any port.

## 4. The visit

    day_salt = HMAC-SHA256(secret, "YYYY-MM-DD")        UTC date of the request
    visit    = lowercase_hex(HMAC-SHA256(day_salt, ip + "\n" + user_agent))[0..16]

`ip` is the client address as text (`203.0.113.9`, `2001:db8::1`), `user_agent`
the header verbatim (empty if absent). Absent address: no `visit`.

The visit cannot be reversed, but it can be tested: whoever holds the secret and
the events can check whether a given address and User-Agent produced a visit on
a given day. Keep the secret out of wherever the events are stored, and rotate
it; a rotation ends every visit at that moment, which is the only cost.

## 5. JSON lines

A sink receives one JSON object per line: the event's keys as received (unknown
ones dropped), `s`, and `time`: the event's time as RFC 3339 with milliseconds,
the event's own `ts` when that is within five minutes of receipt, else the time
of receipt. Cloud Logging and most log agents read `time` as the entry's
timestamp, so beacons that arrive together (after a cold start, say) keep their
order; `s.at` stays the time of receipt in whole seconds. Key order is not
significant.

## 6. App reports and checks (version 2)

Version 1 above is unchanged. Section 6 describes typed reports from installed
programs and their update checks. A report has no identifier and no address;
the collector computes no visit for one.

### 6.1 Transport

| Request | Meaning |
|---|---|
| `POST <path>` (default `/a`) | the body is the JSON of one report, an object; an array is `400`. Cap 64 KB (`413`). |
| `GET <path>/check` | the check's keys as query parameters, nothing else |
| any other method | `405` |

- `Content-Type` is not checked.
- `User-Agent` must be `<app>/<version>`, optionally followed by a space and anything, where `<app>` is the report's `app`; else `403`.
- There is no `Origin`. A request that carries one is `403`.
- `app` must be one the collector allows; else `403`.
- A report with `ch` equal to `source` is `403` and is not written.
- A report with any key the schema does not know, at any depth (including a top-level `s` or `time`), is refused whole with `400`; nothing is trimmed.
- The check is GET only. A POST body with `k` equal to `check` is `400`; a GET with any other `k` is `400`.
- Answers: `204` report accepted, or check with no redirect configured; `302` check, with `Location` set to the app's redirect; `400` malformed, unknown key, bad value; `403`; `413`; `429`; `405`. Every answer carries `cache-control: no-store`. An error body is `{"error": "<reason>"}`.
- A valid check is written before the answer is sent.
- The rate limit is the beacon route's, by address, shared between the routes.

### 6.2 The envelope

Every report and check carries these keys, all required, and no key outside its kind's table.

| Key | Type | Rule |
|---|---|---|
| `v` | integer | `2`; else `unknown version` |
| `app` | string | `^[a-z][a-z0-9_-]{0,31}$` |
| `ver` | string | `^[0-9]+\.[0-9]+\.[0-9]+([-+][0-9A-Za-z.-]+)?$`, at most 32 |
| `bin` | string | `^[a-z][a-z0-9_-]{0,31}$` |
| `os` | string | `macos`, `linux`, `windows` |
| `osv` | integer | `>= 0`, the OS major version |
| `arch` | string | `arm64`, `x86_64` |
| `ch` | string | `^[a-z][a-z0-9_-]{0,31}$`: `release`, `source`, or a name the program adds (`tap`, `direct`) |
| `k` | string | `check`, `session`, `crash` |

### 6.3 check

| Key | Type | Rule |
|---|---|---|
| `rd` | integer, required | `>= -1`: days since the program's last check through this route, rounded down; `-1` the first ever |

A duplicate key in the query is refused (`400`); the last value does not win.

### 6.4 session

| Key | Type | Rule |
|---|---|---|
| `sid` | string, required | `^[0-9a-f]{16,32}$` |
| `len` | integer, required | `>= 0`, seconds |
| `end` | string, required | `quit`, `lost`, `unknown` |
| `iw` | integer, required | `>= 0`, days since the epoch of first launch, a multiple of 7 |
| `rd` | integer, required | `>= -1`, days since the last session report |
| `focus` | object, required | at most 16 keys, `^[a-z][a-z0-9_]{0,31}$`, values integers `>= 0` |
| `ev` | array, required | at most 200 events |
| `ev_dropped` | integer, optional | `>= 0` |
| `ev_rejected` | integer, optional | `>= 0` |

An event:

| Key | Type | Rule |
|---|---|---|
| `n` | string, required | `^[a-z][a-z0-9_]{0,31}$` |
| `t` | integer, required | `>= 0`, seconds since launch |
| `d` | object, optional | at most 6 keys, `^[a-z][a-z0-9_]{0,31}$`; values are strings of at most 64 characters matching `^[A-Za-z0-9_.:/-]*$`, integers, or booleans |

More than 200 events, more than 6 `d` keys or more than 16 `focus` keys is `400`, not `413`.

### 6.5 crash

| Key | Type | Rule |
|---|---|---|
| `inc` | string, required | `^[0-9a-f]{16,32}$` |
| `ck` | string, required | `panic`, `signal` |
| `commit` | string, required | 7 to 40 lowercase hex |
| `pk` | string, required | `^[a-z][a-z0-9_]{0,63}$`, or `other` |
| `mh` | string, optional | 64 lowercase hex, SHA-256 of the message's first line |
| `file` | string, required | `^[A-Za-z0-9_./-]*$`, at most 256 |
| `line` | integer, required | `>= 0` |
| `frames` | array, required | at most 64 pairs of integers `>= 0`: `[image index, offset]` |
| `images` | array, required | at most 32 triples `[name, uuid, size]`: name `^[A-Za-z0-9_.-]{1,64}$`, uuid 32 lowercase hex, size integer `>= 0` |
| `sig` | integer, optional | `>= 0` |
| `up` | string, required | `<1m`, `<10m`, `<1h`, `<1d`, `>=1d` |
| `mem` | string, optional | `normal`, `warn`, `critical`, `unknown` |

### 6.6 Value rules

Every string has a pattern and a cap; there is no free text. A value that fails
its pattern, an integer below its minimum, or a value of the wrong type is
`400`. A path in an error is dotted with no index (`ev.d.x`, `images.uuid`).

### 6.7 Decoration

The collector adds `s` and nothing else:

| Key | Value |
|---|---|
| `sv` | `2` |
| `at` | RFC 3339 UTC time of the request, whole seconds |

No `visit`, `host`, browser, OS from headers, or country.

### 6.8 JSON line

One object per line: the report's keys, `s`, and `time`, the time of receipt as
RFC 3339 with milliseconds (a report carries no clock of its own). Nothing about
the request is written. Key order is not significant.

### 6.9 Fixtures

`spec/fixtures/app/`: `check.json` (`query`), `session.json` and `crash.json`
(valid report bodies), `invalid.json` (`reason`, `status`, and a `report` body
or a `check` query; each report is sent as an `arkos/0.5.0` agent).
