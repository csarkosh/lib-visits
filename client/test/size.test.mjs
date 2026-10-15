import { test } from "node:test";
import assert from "node:assert/strict";
import { statSync } from "node:fs";

const BUDGET = { visits: 2560, vitals: 1024 };

for (const [name, max] of Object.entries(BUDGET)) {
  test(`${name}.min.js is within its ${max} byte budget`, () => {
    const size = statSync(new URL(`../dist/${name}.min.js`, import.meta.url)).size;
    assert.ok(size <= max, `${name}.min.js is ${size} B, over the ${max} B budget`);
  });
}
