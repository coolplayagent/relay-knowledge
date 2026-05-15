import { mkdirSync, readFileSync, writeFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { build } from "esbuild";

const distUrl = new URL("../dist/", import.meta.url);
const assetsUrl = new URL("../dist/assets/", import.meta.url);
const assets = fileURLToPath(assetsUrl);

mkdirSync(assets, { recursive: true });

await build({
  entryPoints: [fileURLToPath(new URL("../src/main.ts", import.meta.url))],
  bundle: true,
  format: "esm",
  platform: "browser",
  target: "es2022",
  outfile: fileURLToPath(new URL("main.js", assetsUrl)),
  sourcemap: false,
});

const styles = [
  readFileSync(new URL("../src/styles.css", import.meta.url), "utf8"),
  readFileSync(new URL("../src/runtime.css", import.meta.url), "utf8"),
  readFileSync(new URL("../src/settings.css", import.meta.url), "utf8"),
  readFileSync(new URL("../src/graph_canvas.css", import.meta.url), "utf8"),
].join("\n");
writeFileSync(new URL("styles.css", assetsUrl), styles);

const html = readFileSync(new URL("../index.html", import.meta.url), "utf8")
  .replace(
    '<script type="module" src="/src/main.ts"></script>',
    '<script type="module" src="/assets/main.js"></script>'
  )
  .replace("</head>", '    <link rel="stylesheet" href="/assets/styles.css" />\n  </head>');

writeFileSync(new URL("index.html", distUrl), html);
