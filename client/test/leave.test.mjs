import { test } from "node:test";
import assert from "node:assert/strict";
import { load, last } from "./harness.mjs";

test("going hidden sends a leave with engaged seconds, scroll depth and reached-the-end", () => {
  const t0 = Date.now;
  Date.now = () => 1_000_000;                                        // frozen before load, so the page's t0 is on this clock
  const { sent, doc, fire, intersect, ctx } = load({ readEnd: true });
  fire("document", "visibilitychange");                              // still visible: no-op path
  ctx.scrollY = 2000; fire("window", "scroll");                       // (2000 + 800) / 4000 = 70%
  intersect();
  Date.now = () => 1_042_000; doc.visibilityState = "hidden"; fire("document", "visibilitychange");
  Date.now = t0;
  const lv = last(sent);
  assert.equal(lv.n, "leave");
  assert.equal(lv.sd, 70);
  assert.equal(lv.re, true);
  assert.ok(lv.e >= 41 && lv.e <= 43, `engaged ${lv.e}`);
});

test("pagehide after hidden does not resend identical values; visible again then hidden sends again", () => {
  const { sent, doc, fire } = load();
  doc.visibilityState = "hidden"; fire("document", "visibilitychange");
  assert.equal(sent.length, 2, "the view plus exactly one leave");
  assert.equal(last(sent).n, "leave");
  const n1 = sent.length;
  fire("window", "pagehide");
  assert.equal(sent.length, n1, "identical leave not resent");
  doc.visibilityState = "visible"; fire("document", "visibilitychange");
  fire("window", "scroll");
  doc.visibilityState = "hidden"; fire("document", "visibilitychange");
  assert.equal(sent.length, n1, "nothing changed, still not resent");
});

test("a page that loads hidden accrues no engaged time until it is seen", () => {
  const { sent, doc, fire } = load({ visibility: "hidden" });
  fire("window", "pagehide");
  assert.equal(sent.length, 1, "a leave with zeros, and no view");
  assert.equal(last(sent).e, 0);
});

test("engaged time accumulates across visible stretches, hidden time is not counted, depth keeps its max", () => {
  const t0 = Date.now;
  try {
    let now = 1_000_000;
    Date.now = () => now;
    const { sent, doc, fire, ctx } = load();
    now += 10_000; doc.visibilityState = "hidden"; fire("document", "visibilitychange");
    assert.equal(last(sent).e, 10);
    now += 30_000; doc.visibilityState = "visible"; fire("document", "visibilitychange");
    ctx.scrollY = 2000; fire("window", "scroll");
    ctx.scrollY = 0; fire("window", "scroll");
    now += 5_000; doc.visibilityState = "hidden"; fire("document", "visibilitychange");
    const lv = last(sent);
    assert.equal(lv.n, "leave");
    assert.equal(lv.e, 15);
    assert.equal(lv.sd, 70);
  } finally { Date.now = t0; }
});
