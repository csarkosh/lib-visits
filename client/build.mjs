// Minify the two scripts into dist/. Minify only: no transpile, no bundle, so the
// shipped file is the source with the whitespace and names squeezed out.
import { build } from "esbuild";
import { mkdirSync, statSync } from "node:fs";

mkdirSync(new URL("./dist/", import.meta.url), { recursive: true });
for (const name of ["visits", "vitals"]) {
  await build({
    entryPoints: [new URL(`./src/${name}.js`, import.meta.url).pathname],
    outfile: new URL(`./dist/${name}.min.js`, import.meta.url).pathname,
    minify: true,
    target: "es2018",
    legalComments: "none",
    logLevel: "error",
  });
  const size = statSync(new URL(`./dist/${name}.min.js`, import.meta.url)).size;
  console.log(`${name}.min.js ${size} B`);
}
