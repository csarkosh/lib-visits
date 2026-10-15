import { test } from "node:test";
import assert from "node:assert/strict";
import { load, last } from "./harness.mjs";

test("a visible page sends one view with the spec's keys and nothing else from the query", () => {
  const { sent } = load({ referrer: "https://www.google.com/search?q=secret#x" });
  assert.equal(sent.length, 1);
  const v = last(sent);
  assert.equal(v.v, 1);
  assert.equal(v.n, "view");
  assert.match(v.id, /^[a-z0-9]{10}$/);
  assert.equal(v.p, "/research/a-doc");
  assert.equal(v.t, "A doc");
  assert.equal(v.r, "https://www.google.com/search", "no query or fragment in the referrer");
  assert.equal(v.us, "news");
  assert.equal("token" in v, false, "the rest of the query never leaves the page");
  assert.equal(v.nt, "navigate");
  assert.deepEqual([v.sw, v.sh, v.vw, v.vh, v.dpr], [1512, 982, 1200, 800, 2]);
  assert.equal(v.th, "dark");
  assert.equal(v.tz, "America/New_York");
  assert.equal(v.lg, "en-US");
  assert.equal("rm" in v, false, "false booleans are omitted");
  assert.equal("b" in v, false, "a bot code of 0 is omitted");
});

test("a hidden page waits until it is visible", () => {
  const { sent, doc, fire } = load({ visibility: "hidden" });
  assert.equal(sent.length, 0);
  doc.visibilityState = "visible";
  fire("document", "visibilitychange");
  assert.equal(sent.length, 1);
  fire("document", "visibilitychange");
  assert.equal(sent.length, 1, "counted once");
});

test("a same-host canonical replaces the path; another host or a non-http scheme does not", () => {
  assert.equal(last(load({ canonical: "https://www.example.com/canonical-path?x=1" }).sent).p, "/canonical-path");
  assert.equal(last(load({ canonical: "https://other.example/canonical-path" }).sent).p, "/research/a-doc");
  assert.equal(last(load({ canonical: "javascript:alert(1)" }).sent).p, "/research/a-doc");
});

test("the bot code names the automation", () => {
  assert.equal(last(load({ webdriver: true }).sent).b, 1);
});

test("iframes, local pages and opted-out browsers are not counted", () => {
  assert.equal(load({ framed: true }).sent.length, 0);
  assert.equal(load({ href: "http://localhost:5173/", hostname: "localhost" }).sent.length, 0);
  assert.equal(load({ href: "http://localhost:5173/", hostname: "localhost", dataset: { visits: "/v", visitsLocal: "" } }).sent.length, 1);
  assert.equal(load({ href: "file:///index.html", hostname: "" }).sent.length, 0);
  assert.equal(load({ storage: { visits: "off" } }).sent.length, 0);
});

test("#visits-off writes the opt-out and stops; #visits-on clears it", () => {
  const off = load({ href: "https://example.com/#visits-off" });
  assert.equal(off.sent.length, 0);
  assert.equal(off.ctx.localStorage.visits, "off");
  const on = load({ href: "https://example.com/#visits-on", storage: { visits: "off" } });
  assert.equal(on.sent.length, 1);
  assert.equal("visits" in on.ctx.localStorage, false);
});

test("GET mode drops absent values instead of sending the string undefined", () => {
  const { sent } = load({ dataset: { visits: "https://stats.example/v", visitsMethod: "get" } });
  assert.equal(sent.length, 1);
  assert.equal(sent[0].method, "GET");
  assert.equal(sent[0].url.includes("undefined"), false);
  const q = new URL(sent[0].url).searchParams;
  for (const k of ["rm", "tp", "b"]) assert.equal(q.has(k), false, k);
  assert.equal(q.get("n"), "view");
});

test("blocked storage does not stop the view", () => {
  assert.equal(load({ storageThrows: true }).sent.length, 1);
});

test("a missing browser API drops the view without throwing into the page", () => {
  assert.doesNotThrow(() => load({ noPerformance: true }));
});
