import { test } from "node:test";
import assert from "node:assert/strict";
import { load, last, emit } from "./harness.mjs";

const hide = ({ doc, fire }) => { doc.visibilityState = "hidden"; fire("document", "visibilitychange"); };

test("sends lcp (last entry), cls (largest session window), inp and ttfb once on hidden", () => {
  const h = load({ scripts: ["visits", "vitals"] });
  emit(h.ctx, "largest-contentful-paint", [{ startTime: 800 }, { startTime: 1234.4 }]);
  // window A: 0.05 at t=1000 and 0.02 at t=1500 (gap < 1 s); window B: 0.1 at t=9000; input-caused shift ignored
  emit(h.ctx, "layout-shift", [
    { startTime: 1000, value: 0.05 }, { startTime: 1500, value: 0.02 },
    { startTime: 9000, value: 0.1 }, { startTime: 9100, value: 0.5, hadRecentInput: true },
  ]);
  emit(h.ctx, "event", [
    { interactionId: 1, duration: 56 }, { interactionId: 1, duration: 40 }, { interactionId: 2, duration: 120 },
  ]);
  h.ctx.performance.interactionCount = 2;
  hide(h);
  const v = last(h.sent);
  assert.equal(v.n, "vitals");
  assert.deepEqual(v.d, { lcp: 1234, cls: 0.1, inp: 120, ttfb: 180 });
  const n = h.sent.length;
  h.fire("window", "pagehide");
  assert.equal(h.sent.filter((s) => JSON.parse(s.body).n === "vitals").length, 1, "once");
});

test("with more than 50 interactions the pick moves down the sorted list", () => {
  const h = load({ scripts: ["visits", "vitals"] });
  emit(h.ctx, "event", Array.from({ length: 60 }, (_, i) => ({ interactionId: i + 1, duration: 100 + i })));
  h.ctx.performance.interactionCount = 60;
  hide(h);
  assert.equal(last(h.sent).d.inp, 158, "index floor(60/50)=1 of the ten longest, sorted descending: 159, 158, ...");
});

test("observes event timing with the 40 ms threshold, and sends nothing without the core", () => {
  const h = load({ scripts: ["visits", "vitals"] });
  assert.equal(h.ctx.perfObservers.find((o) => o.type === "event").opts.durationThreshold, 40);
  const alone = load({ scripts: ["vitals"] });
  hide(alone);
  assert.equal(alone.sent.length, 0);
});
