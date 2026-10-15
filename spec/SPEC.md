# lib-visits wire specification, version 1

## 1. Transport

A client sends one event per request to the collector's path (default `/v`).

- `POST`: the body is the JSON of one event, or a JSON array of at most 50 events.
  `navigator.sendBeacon(path, string)` sends it as `text/plain`, so no preflight.
- `GET`: the event's keys as query parameters. Numbers and booleans are their
  decimal and `true`/`false` spellings; `d` is JSON-encoded as one parameter.

Caps: body 16 KB. The origin is the `Origin` header, else the host of `Referer`,
and must be allowlisted. Statuses: `204` accepted, `400` malformed, `403` origin
not allowed, `413` too large or more than 50 events, `429` rate limited, `405`
other methods. Every answer carries `cache-control: no-store`. The body of an
error is `{"error": "<reason>"}`.

The limiter is a courtesy filter, not a boundary: the route is public. The client address is only as trustworthy as `--trusted-proxies` is accurate (a value higher than the real proxy count lets a client choose its own address), and when the limiter's table is full of fresh addresses, newcomers are served untracked rather than real visitors refused. A host under a flood answers it at its reverse proxy or CDN.

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
| `b` | integer | | bot marker: 1 webdriver, 2 PhantomJS, 3 Nightmare, 4 Selenium |
| `e` | integer | | engaged seconds |
| `sd` | integer | | max scroll depth 0 to 100 |
| `ev` | string | 64 | the named event of a click |
| `u` | string | 2048 | origin plus path of an outbound click |
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
| `os` | `Windows`, `macOS`, `iOS`, `Android`, `Linux`, `ChromeOS`, `other` |
| `bot` | `true` when `b` is non-zero or the User-Agent contains (case-insensitive) one of: bot, crawl, spider, slurp, headless, lighthouse, pagespeed, gtmetrix, facebookexternalhit, preview |
| `rk` | `direct` (no referrer), `internal` (referrer host equals `host`), `external` |
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
ones dropped) plus `s`. Key order is not significant.
