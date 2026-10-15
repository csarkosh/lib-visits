# lib-visits: working context

A cookieless web analytics library: `client/` (one script under 3 KB, an optional
Web Vitals module) and `crates/` (a Rust collector, one static binary). Design:
`docs/superpowers/specs/2026-10-15-lib-visits-design.md`. The wire format is
`spec/SPEC.md`; `spec/fixtures/*.json` are read by both test suites.

- `cargo test` and, in `client/`, `npm test` are the gates. Run the targeted suite for
  what changed; never run anything heavy in the background.
- Size gates: `visits.min.js` ≤ 3,072 B (4 B of headroom at v0.1), `vitals.min.js` ≤ 1,024 B. The gates do not move: a client-side correctness fix that needs bytes is made on the server instead, which validates and may truncate.
- No cookies, no storage (except the visitor's `#visits-off`), no fingerprinting API.
- No em-dashes. Conventional Commits. Stage paths explicitly, never `git add -A`.
- `docs/superpowers/` holds specs and plans and is never published.
