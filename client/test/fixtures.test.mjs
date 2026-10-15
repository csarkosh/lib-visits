// The shared wire fixtures must describe what the client really sends: every key
// it emits is in the fixture's `in` with the same type, and every required key
// of the fixture is emitted.
import { test } from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { load, last, emit } from "./harness.mjs";

const fixture = (name) => JSON.parse(readFileSync(new URL(`../../spec/fixtures/${name}.json`, import.meta.url)));
const OPTIONAL = new Set(["nt", "rm", "tp", "b", "us", "um", "uc", "ut", "uo", "cr", "re", "r", "t", "lg", "tz"]);
const anchor = (href, dataset = {}) => { const u = new URL(href); return { host: u.host, origin: u.origin, pathname: u.pathname, dataset, closest() { return this; } }; };
const hide = ({ doc, fire }) => { doc.visibilityState = "hidden"; fire("document", "visibilitychange"); };

const produce = {
  view: () => last(load().sent),
  "view-internal": () => last(load({ referrer: "https://www.example.com:443/" }).sent),
  leave: () => { const h = load(); hide(h); return last(h.sent); },
  "click-named": () => { const h = load(); h.fire("window", "click", { target: anchor("https://example.com/games", { visitsEvent: "out-github" }) }); return last(h.sent); },
  "click-outbound": () => { const h = load(); h.fire("window", "click", { target: anchor("https://github.com/csarkosh") }); return last(h.sent); },
  vitals: () => {
    const h = load({ scripts: ["visits", "vitals"] });
    emit(h.ctx, "largest-contentful-paint", [{ startTime: 1234 }]);
    hide(h);
    return last(h.sent);
  },
};

const typeOf = (v) => (v !== null && typeof v === "object" ? "object" : typeof v);

for (const [name, make] of Object.entries(produce)) {
  test(`the ${name} fixture matches what the client sends`, () => {
    const expected = fixture(name).in;
    const got = make();
    for (const k of ["v", "n", "p"]) assert.ok(k in got, `client sends ${k}`);
    assert.equal(got.n, expected.n);
    for (const k of Object.keys(got)) {
      if (k === "id") continue;
      assert.ok(k in expected, `the fixture lacks "${k}", which the client sends`);
      assert.equal(typeOf(got[k]), typeOf(expected[k]), `type of ${k}`);
    }
    for (const k of Object.keys(expected)) {
      if (OPTIONAL.has(k) || k === "unknown") continue;  // "unknown" is the fixture's deliberate key the server must drop
      assert.ok(k in got, `the client does not send "${k}", which the fixture requires`);
    }
  });
}
