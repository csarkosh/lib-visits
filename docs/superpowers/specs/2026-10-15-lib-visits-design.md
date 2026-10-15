# lib-visits: design

*2026-10-15. Approved in conversation; this is the written form.*

## 1. What it is

A cookieless web analytics library in two parts: a client script of under 2.5 KB that
any page can load, and a server written in Rust that any Linux box can run as one static
binary. It counts visits and reconstructs the path a visit took through a site, without
cookies, without storage, and without fingerprinting. It ships no dashboard and no
database: decorated events come out as JSON lines, and where they go is the host's choice.

It replaces GoatCounter on csarko.sh (the trigger: a GoatCounter outage on 2026-10-15 took
five dashboard cards down), and it is built as a product from day one, so that making it
public later is flipping a repository to public. Licence: GPL-3.0-only.

### What it will not do

- **No cookies, no storage.** The only exception is the visitor's own opt-out (section
  3.6), written only when the visitor asks for it.
- **No fingerprinting.** Nothing from canvas, WebGL, audio, fonts or the high-entropy
  client hints. A visit is a stateless server-side hash that cannot survive the day
  (section 4.4), which is the same ceiling GoatCounter chose, for the same reason: it is
  the line past which a cookie banner is owed.
- **No hosted service.** The library hosts nothing. A host runs the binary.

### Who it is for

First, csarko.sh and its admin dashboard (its wiring is a separate spec in that
repository). Then anyone who wants GoatCounter's numbers plus the journey, on their own
box, with one binary and one script tag.

## 2. The repository

```
lib-visits/
  client/
    src/visits.js         the core
    src/vitals.js         the optional Web Vitals module
    dist/                 minified builds, committed at each tag
    test/                 node:test, with a hand-made fake window
    build.mjs             esbuild minify + the size gate
  crates/
    visits-core/          pure: validate, decorate, hash, classify; no I/O
    visits/               the `visits` binary: `serve`, sinks, limiter
  spec/
    SPEC.md               the wire format and the decorations, normative
    fixtures/*.json       examples both test suites read
  Dockerfile              rust:alpine builder, FROM scratch final
  README.md               a brief marketing page and a setup guide (section 7)
  AGENTS.md               working context
  LICENSE                 GPL-3.0-only
  Cargo.toml              workspace, members crates/*, edition 2021, stable
```

Conventions follow `ide-sarkos`: a Cargo workspace with `crates/*`, edition 2021,
`license = "GPL-3.0-only"` in the workspace package, `rust-toolchain.toml` on stable,
`serde` and `anyhow` as the common dependencies. The client has one devDependency,
esbuild, and no runtime dependency.

## 3. The client

### 3.1 Loading

```html
<script src="/visits.js" data-visits="/v"></script>
```

`data-visits` is the endpoint and the only required setting. Other attributes:

| Attribute | Default | Meaning |
|---|---|---|
| `data-visits-method` | `post` | `get` sends each event as query parameters (section 3.5) |
| `data-visits-local` | absent | present: count `localhost`, private addresses and `file:` pages too |

No global configuration object, no JSON in an attribute. The script runs on load and
exposes one object, `window.visits`, with `send(name, data)` and `id` (section 3.4).

### 3.2 Three events, one id

Each page load draws a random id (ten base-36 characters from `Math.random`) that lives
only in that page's memory. It joins the page's events to each other and to nothing else.
Every event carries `v: 1`, `n` (its name), `id`, and `p` (the path).

**`view`**, once, the first time the page is visible. A prerendered or background tab waits
for `visibilitychange`, as GoatCounter's `count.js` does, so a tab that is never looked at
is never counted.

| Key | Value |
|---|---|
| `p` | the path: `location.pathname`, or the `<link rel=canonical>` path when its host is this host (`www.` ignored); never the query string |
| `t` | `document.title`, at most 200 characters |
| `r` | `document.referrer` without its query or fragment (a same-site referrer carries the previous page's query, which may hold a token), at most 2 KB; the server classifies it |
| `us` `um` `uc` `ut` `uo` `cr` | the campaign parameters `utm_source`, `utm_medium`, `utm_campaign`, `utm_term`, `utm_content`, `ref`, each only when present, each at most 100 characters. The rest of the query string is never sent, so a token in a URL never lands in a log |
| `nt` | navigation type from `PerformanceNavigationTiming`: `navigate`, `reload`, `back_forward`, `prerender` |
| `sw` `sh` | `screen.width`, `screen.height` |
| `vw` `vh` | `innerWidth`, `innerHeight` |
| `dpr` | `devicePixelRatio`, two decimals |
| `th` | `dark` or `light`, from `prefers-color-scheme` |
| `rm` | `true` when `prefers-reduced-motion: reduce` |
| `tp` | `true` when `navigator.maxTouchPoints > 0` |
| `tz` | `Intl.DateTimeFormat().resolvedOptions().timeZone` |
| `lg` | `navigator.language` |
| `b` | bot marker: `0` none, `1` `navigator.webdriver`, `2` PhantomJS, `3` Nightmare, `4` Selenium |

**`leave`**, when the tab goes hidden and again on `pagehide`. It may be sent more than once
for a page (hidden, visible again, hidden): each carries cumulative values and the query
takes the last one per `id`. Two identical `leave`s in a row are not resent.

| Key | Value |
|---|---|
| `e` | engaged seconds: whole seconds the page was visible, accumulated across `visibilitychange` |
| `sd` | max scroll depth, 0 to 100: the furthest `(scrollY + innerHeight) / scrollHeight` seen |
| `re` | `true` once the first `[data-read-end]` element has intersected the viewport |

**`click`**, on `click` and `auxclick` of an `<a>` whose host is not this host, or of any
element carrying `data-visits-event`.

| Key | Value |
|---|---|
| `ev` | the `data-visits-event` value, when present (the stable name of a tracked action) |
| `u` | otherwise, the link's origin plus path, never its query or hash |

Event names in `data-visits-event` are identifiers, not copy: renaming a button must not
rename its series.

### 3.3 The Web Vitals module

`vitals.js` is a second script that loads after the core and uses `visits.send`. It
registers `PerformanceObserver`s at load and sends one `vitals` event, once, on the same
hidden signal as `leave`. As with every event sent through `visits.send`, its values ride
under `d`:

| Key | Value |
|---|---|
| `lcp` | largest-contentful-paint: the last entry's `startTime`, ms |
| `cls` | layout-shift entries without recent input, summed by session window (1 s gap, 5 s cap), the largest window, three decimals |
| `inp` | event-timing entries with an `interactionId` and `durationThreshold` 40: the longest ten are kept and the one at index `floor(interactions / 50)` reported, ms (the web-vitals library's estimate of the 98th percentile) |
| `ttfb` | `responseStart` of the navigation entry, ms |

It is a separate file so that the core stays honest about its size and so that a host who
does not want field vitals never loads them.

### 3.4 The public surface

`window.visits = { send, id }`. `send(name, data)` sends a custom event: `name` matches
`^[a-z][a-z0-9_-]{0,31}$` and `data` is a flat object of at most ten primitive values,
strings at most 200 characters, carried as `d`. The vitals module is its first caller.

### 3.5 Transport

One request per event. `post` (the default): `navigator.sendBeacon(endpoint, json)`, whose
body is `text/plain`, a simple request that needs no preflight and survives unload;
`fetch` with `keepalive: true` when `sendBeacon` is missing. `get`: `fetch(endpoint + "?" +
params, { keepalive: true })`, for a host whose web server logs requests and runs nothing
(section 4.8); `sendBeacon` cannot GET. There is no `<img>` fallback: a browser without
`sendBeacon` or `fetch` is not counted, silently.

### 3.6 What it refuses to count

- Iframes (`location !== parent.location`).
- `localhost`, `127.`, `10.`, `172.16-31.`, `192.168.`, `0.0.0.0` and `file:` pages, unless `data-visits-local`.
- A page with `navigator.webdriver` or a headless marker is counted with its `b` code; the server flags it.
- A browser where the visitor has opted out: loading any page with `#visits-off` sets
  `localStorage.visits = "off"` and the script stops; `#visits-on` removes it. This is the
  one piece of storage, written only at the visitor's request, as GoatCounter's toggle is.

Everything is wrapped so that an old browser throws nothing into the page's console.

### 3.7 Size

The build fails if `visits.min.js` exceeds **2,560 bytes** or `vitals.min.js` exceeds
**1,024 bytes**, minified, before compression. The budget is a test, not a goal: the
README quotes the real numbers.

## 4. The server

### 4.1 The binary

`visits serve` listens for beacons, validates and decorates them, and hands them to a
sink. One static binary per platform (`x86_64-unknown-linux-musl`, `aarch64-unknown-linux-musl`,
macOS, Windows), a few megabytes, a few megabytes of memory at idle. `visits log`
is reserved for the access-log mode (section 4.8) and is not in v1.

Dependencies: `axum` and `tokio` (the server), `serde` and `serde_json`, `hmac` and
`sha2`, `clap`, `anyhow`; `maxminddb` behind the cargo feature `geoip`, off by default.
The rate limiter is our own forty lines. No tracing stack: the sink is the log, and
operational messages go to stderr.

### 4.2 Configuration

Flags, each with an environment fallback:

| Flag | Env | Default | Meaning |
|---|---|---|---|
| `--listen` | `VISITS_LISTEN` | `0.0.0.0:8080` | |
| `--path` | `VISITS_PATH` | `/v` | the beacon route |
| `--origin` (repeatable) | `VISITS_ORIGINS` (comma-separated) | required | the sites allowed to post |
| `--secret-env` | | `VISITS_SECRET` | the variable holding the hash secret; absent: no `visit` field and a warning at start |
| `--sink` | `VISITS_SINK` | `stdout` | `stdout`, or `file:<path>` (append, one JSON line an event) |
| `--geoip` | `VISITS_GEOIP` | none | path to an `.mmdb` city or country database (feature `geoip`) |
| `--trusted-proxies` | `VISITS_TRUSTED_PROXIES` | `0` | how many proxies sit in front; the client address is that many entries from the right of `X-Forwarded-For`, or the peer address at 0 |
| `--limit` | `VISITS_LIMIT` | `60/1` | per-address token bucket: burst and refill a second |
| `--log-ip` | | off | setup only: print each request's resolved address to stderr, to confirm `--trusted-proxies`; never left on, since it writes the IP the design otherwise never writes |

`GET /healthcheck` answers `200 ok`, for probes. Not `/healthz`: on Cloud Run, Google's
frontend answers that exact path itself.

### 4.3 The request

`POST <path>` with a JSON body, or `GET <path>?<params>` in `get` mode. The body or the
query is one event, or an array of events (at most 50, so a later batching client needs
no server change). Caps: body 16 KB; strings as in section 3; `d` at most ten keys.
Unknown keys are dropped; a wrong type fails the request.

The origin is the `Origin` header, else the host of `Referer` (a same-origin GET sends no
`Origin`). It must be in the allowlist.

| Status | When |
|---|---|
| `204` | accepted; the sink's own failure is logged to stderr, never returned |
| `400` | malformed body, unknown version, bad type, over a string cap |
| `403` | origin not allowed |
| `413` | body over 16 KB or more than 50 events |
| `429` | the address is over its bucket; `retry-after: 1` |

Every answer carries `cache-control: no-store`. A cross-origin accepted request carries
`access-control-allow-origin` for its origin.

### 4.4 The visit hash

```
day_salt = HMAC-SHA256(secret, "YYYY-MM-DD")        UTC date of the request
visit    = hex(HMAC-SHA256(day_salt, ip + "\n" + user_agent))[0..16]
```

Stateless: every instance and every cold start agree, with no session table. The IP is an
input and is never written. Tomorrow's salt is unrelated to today's, so nothing links two
days; this is the ceiling, by design.

### 4.5 The decoration

Added as the object `s` beside the client's keys, never over them:

| Key | Value |
|---|---|
| `sv` | decoration version, `1` |
| `at` | the server's RFC 3339 time; the client sends no clock |
| `visit` | section 4.4, when a secret is configured |
| `host` | the resolved origin's host |
| `bn` `bv` | browser family and major version: from `Sec-CH-UA` when present, else a small `User-Agent` regex set (Chrome, Edge, Firefox, Safari, Opera, Samsung Internet, Chrome iOS, Firefox iOS; everything else `other`) |
| `os` | `Sec-CH-UA-Platform` when present, else from the User-Agent: Windows, macOS, iOS, Android, Linux, ChromeOS, `other` |
| `bot` | `true` when the client's `b` is non-zero or the User-Agent matches the crawler list (`bot`, `crawl`, `spider`, `slurp`, `headless`, `lighthouse`, `pagespeed`, `gtmetrix`, `facebookexternalhit`, `preview`, case-insensitive) |
| `rk` | the referrer classified: `direct` (empty), `internal` (its host is `host`), `external` |
| `rp` | internal: the referrer's path, the journey's previous step |
| `rh` | external: the referrer's host, `www.` stripped |
| `lg` | `Accept-Language`'s first tag, only when the client sent no `lg` |
| `cc` | ISO 3166-1 alpha-2 country from the GeoIP lookup, when configured |

Rows flagged `bot` are kept and flagged, not dropped; a query filters them.

### 4.6 Sinks

A sink receives decorated events, one JSON object a line, keys exactly as above. `stdout`
is the default and the reference: pipe it anywhere. `file:<path>` appends. A sink's error
is written to stderr and the request still answers `204`: the browser cannot act on a
failure, and an endpoint that answers `5xx` to beacons only fills a log. SQLite is the
expected v2 sink.

### 4.7 Crates

`visits-core` is pure: `validate(&[u8] | &QueryString) -> Result<Vec<Event>>`,
`decorate(Event, &RequestFacts) -> Event`, `visit_hash(secret, date, ip, ua) -> String`,
`classify_referrer`, `browser_of`, `os_of`, `is_crawler`. `RequestFacts` is the handful of
header values and the resolved address, built by the binary, so the core has no HTTP type
in it and its tests read `spec/fixtures/*.json`: for each fixture, the request facts, the
event in, the event out. The client's tests read the same fixtures for the event-in side,
so the wire format has one set of examples and both ends are checked against it.

`visits` holds `main`, the `clap` definitions, the axum router, the token bucket keyed by
the resolved address, the sinks, and the optional GeoIP reader.

### 4.8 The access-log mode (reserved)

The `get` transport exists so that a host can serve `<path>` as a static `204` from nginx,
Caddy or a CDN and let the access log be the event store; `visits log --format <nginx|caddy|cloudfront>`
would then run the same `decorate` over the log, offline, and emit the same JSON lines.
Not in v1. The README names the zero-install floor plainly: a static `204` and your own
log tooling, with no decoration, is a valid setup, and the raw log then holds the IP for
as long as the log is kept.

## 5. Testing

- **Client:** `node --test client/test`, against a fake `window`, `document`, `navigator`,
  `screen` and `performance` small enough to read in one sitting. Each event's keys, the
  canonical rule, the campaign filter, the wait-for-visible, the opt-out, the `get`
  encoding, the vitals arithmetic (CLS windows, INP index). The size gate is a test.
- **Core:** `cargo test` over the fixtures, plus unit tests for the hash (known vectors),
  the UA set, the crawler list, the referrer classes, every validation cap.
- **Binary:** an integration test starts `serve` on a free port with a capturing sink,
  posts each fixture and asserts the status, the headers and the sink's line; the
  limiter's `429`; the origin's `403`; the GET mode.
- No browser automation in v1: the client is 1.5 KB of code against a stable DOM surface,
  and csarko.sh's own checks exercise it on real pages.

## 6. Build and release

`npm run build` in `client/` minifies with esbuild and runs the gate; `cargo build --release`
builds the binary; the `Dockerfile` builds a static musl binary in `rust:alpine` and
copies it into `FROM scratch`, so the image is the binary. Tags `v0.1.0`, `v0.2.0`, with
`client/dist/` committed at each tag so a consumer can vendor a file by tag. GitHub
releases with prebuilt binaries come when the repository goes public.

## 7. The README

It is a brief marketing page, then a setup guide that fits on one screen; brevity is the
feature, because a short guide is the one people try. In order:

1. One line on what it is: visits and journeys, no cookies, no fingerprinting, one script
   under 2.5 KB and one binary of a few MB that idles at a few MB.
2. Three or four short reasons, with the real numbers from the build.
3. **Setup**, three steps, nothing else between them:
   1. Run it: `VISITS_SECRET=$(openssl rand -hex 32) ./visits serve --origin https://example.com`
   2. Add the tag: `<script src="/visits.js" data-visits="https://stats.example.com/v"></script>`
   3. Watch the lines: `{"v":1,"n":"view","p":"/", ... "s":{"visit":"3f9a...","bn":"Firefox", ...}}`
4. "Put it behind your site": the two-line nginx `location` that proxies `/v`, so the
   beacon is same-origin.
5. **Running it in the cloud**: one high-level system diagram each for GCP, AWS and
   Azure, in Mermaid (GitHub renders it; no image files to keep in step), each the same
   shape so the reader sees that only the names change: the browser, the site's host or
   CDN rewriting the beacon path to the collector, the `visits` container, its stdout
   into the provider's log service, and the query layer a dashboard reads.
   - GCP: Firebase Hosting or Cloud CDN → Cloud Run → Cloud Logging → a Log Analytics
     bucket linked as a BigQuery dataset.
   - AWS: CloudFront → ECS on Fargate or App Runner → CloudWatch Logs → Logs Insights,
     or a subscription into S3 and Athena.
   - Azure: Front Door or Static Web Apps → Container Apps → Azure Monitor → a Log
     Analytics workspace (KQL).
   Each diagram has a two-line caption: what the host pays for at idle (nothing, in all
   three, with scale-to-zero) and where the IP stops (the collector; the logs never hold
   it). Then the bare-metal row: nginx or Caddy in front of the binary, logs to a file.
6. Where to go next: the sinks, GeoIP, the Web Vitals module, `SPEC.md` for another
   language, the opt-out, what is deliberately not collected.

Nothing in the README names csarko.sh's infrastructure; the GCP diagram is generic even
though it is the shape csarko.sh uses.

## 8. Out of scope for v1

A dashboard, any database sink, the access-log CLI, prebuilt release binaries, batching
in the client, a Cloudflare Worker port, Do Not Track or Global Privacy Control handling
(the counting is anonymous; revisit if a host asks).
