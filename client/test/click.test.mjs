import { test } from "node:test";
import assert from "node:assert/strict";
import { load, last } from "./harness.mjs";

const anchor = (href, dataset = {}) => { const u = new URL(href); return { host: u.host, origin: u.origin, pathname: u.pathname, dataset, closest() { return this; } }; };

test("a click on a link to another host sends its origin and path, never its query", () => {
  const { sent, fire } = load();
  fire("window", "click", { target: anchor("https://github.com/csarkosh?tab=repos#x") });
  assert.deepEqual(last(sent), { ...last(sent), n: "click", u: "https://github.com/csarkosh" });
});

test("a same-host link is not a click; a named element is, by its name", () => {
  const { sent, fire } = load();
  const before = sent.length;
  fire("window", "click", { target: anchor("https://example.com/games") });
  assert.equal(sent.length, before);
  fire("window", "auxclick", { target: anchor("https://example.com/games", { visitsEvent: "nav-games" }) });
  assert.equal(last(sent).ev, "nav-games");
  assert.equal("u" in last(sent), false);
});

test("visits.send wraps the payload as d and refuses a bad name", () => {
  const { sent, ctx } = load();
  ctx.visits.send("vitals", { lcp: 1234 });
  assert.deepEqual(last(sent).d, { lcp: 1234 });
  assert.equal(last(sent).id, ctx.visits.id);
  const before = sent.length;
  ctx.visits.send("Bad Name", {});
  assert.equal(sent.length, before);
  ctx.visits.send(); assert.equal(sent.length, before);
  ctx.visits.send(null, {}); assert.equal(sent.length, before);
  ctx.visits.send(42, {}); assert.equal(sent.length, before);
});

test("in get mode the event is query parameters on one fetch, d JSON-encoded", () => {
  const { sent, ctx } = load({ dataset: { visits: "https://stats.example/v", visitsMethod: "get" } });
  assert.equal(sent[0].method, "GET");
  const u = new URL(sent[0].url);
  assert.equal(u.searchParams.get("n"), "view");
  assert.equal(u.searchParams.get("sw"), "1512");
  ctx.visits.send("vitals", { lcp: 1 });
  assert.equal(new URL(sent.at(-1).url).searchParams.get("d"), '{"lcp":1}');
});

test("a click with no target, or a target without closest, throws nothing and sends nothing", () => {
  const { sent, fire } = load();
  const before = sent.length;
  fire("window", "click", { target: {} });
  fire("window", "click", {});
  assert.equal(sent.length, before);
});

test("sendBeacon returning false falls through to fetch keepalive POST", () => {
  const { sent } = load({ beaconReturns: false });
  assert.equal(sent[0].method, "beacon");
  assert.equal(sent[1].method, "POST");
  assert.equal(sent[1].keepalive, true);
  assert.equal(sent[1].body, sent[0].body);
});

test("a browser without sendBeacon uses fetch", () => {
  const { sent } = load({ noBeacon: true });
  assert.equal(sent.length, 1);
  assert.equal(sent[0].method, "POST");
});
