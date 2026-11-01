import { test } from "node:test";
import assert from "node:assert/strict";
import { load, last } from "./harness.mjs";

// A single-page app tells the script when its route changed: `visits.page()`.
const go = (ctx, pathname) => { ctx.location.pathname = pathname; ctx.location.href = `https://example.com${pathname}`; ctx.visits.page(); };

test("page() on a new path leaves the page before and views the new one as navigated to from it", () => {
  const t0 = Date.now;
  Date.now = () => 1_000_000;
  const { sent, ctx } = load();
  const first = last(sent);
  Date.now = () => 1_012_000;
  go(ctx, "/party/abc");
  Date.now = t0;
  assert.equal(sent.length, 3, "the first view, its leave, the new view");
  const [, leave, view] = sent.map((s) => JSON.parse(s.body));
  assert.equal(leave.n, "leave");
  assert.equal(leave.id, first.id, "the leave belongs to the page before");
  assert.equal(leave.p, "/research/a-doc");
  assert.equal(leave.e, 12);
  assert.equal(view.n, "view");
  assert.notEqual(view.id, first.id, "a new page load id");
  assert.match(view.id, /^[a-z0-9]{10}$/);
  assert.equal(view.p, "/party/abc");
  assert.equal(view.r, "https://example.com/research/a-doc", "the referrer is the page before");
  assert.equal(view.nt, "spa");
  assert.equal(view.t, "A doc");
});

test("page() on the path already counted sends nothing", () => {
  const { sent, ctx } = load();
  ctx.visits.page();
  assert.equal(sent.length, 1);
  ctx.location.search = "?x=1";
  ctx.visits.page();
  assert.equal(sent.length, 1, "a query change is not a page");
});

test("after page() the engaged time, depth and read-end start over, and later beacons carry the new id", () => {
  const t0 = Date.now;
  Date.now = () => 1_000_000;
  const { sent, ctx, doc, fire, intersect } = load({ readEnd: true });
  ctx.scrollY = 2000; fire("window", "scroll");
  intersect();
  Date.now = () => 1_010_000;
  go(ctx, "/credits");
  const view = last(sent);
  Date.now = () => 1_013_000;
  doc.visibilityState = "hidden"; fire("document", "visibilitychange");
  Date.now = t0;
  const leave = last(sent);
  assert.equal(leave.n, "leave");
  assert.equal(leave.id, view.id);
  assert.equal(leave.e, 3, "engaged since the new page, not since the load");
  assert.equal(leave.sd, 0, "depth starts over");
  assert.equal("re" in leave, false, "read-end starts over");
  const n = sent.length;
  ctx.visits.send("tipped", { door: "arkos" });
  assert.equal(last(sent).id, view.id, "custom events carry the new id");
  assert.equal(sent.length, n + 1);
});

test("page() while the page before is hidden sends its leave once and the new view", () => {
  const { sent, ctx, doc, fire } = load();
  doc.visibilityState = "hidden"; fire("document", "visibilitychange");
  const before = sent.length;
  go(ctx, "/settings");
  assert.equal(sent.length, before + 1, "no second leave for the hidden page, one view");
  assert.equal(last(sent).p, "/settings");
});
