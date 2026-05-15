import { mkdirSync, readFileSync, writeFileSync } from "node:fs";
import { build } from "esbuild";

const dist = new URL("../dist/", import.meta.url);
const assets = new URL("../dist/assets/", import.meta.url);

mkdirSync(assets, { recursive: true });

await build({
  entryPoints: [new URL("../src/main.ts", import.meta.url).pathname],
  bundle: true,
  format: "esm",
  platform: "browser",
  target: "es2022",
  outfile: new URL("main.js", assets).pathname,
  sourcemap: false,
});

const styles = [
  readFileSync(new URL("../src/styles.css", import.meta.url), "utf8"),
  readFileSync(new URL("../src/graph_canvas.css", import.meta.url), "utf8"),
].join("\n");
writeFileSync(new URL("styles.css", assets), styles);

const html = readFileSync(new URL("../index.html", import.meta.url), "utf8")
  .replace(
    '<script type="module" src="/src/main.ts"></script>',
    '<script type="module" src="/assets/main.js"></script>'
  )
  .replace("</head>", '    <link rel="stylesheet" href="/assets/styles.css" />\n  </head>');

writeFileSync(new URL("index.html", dist), html);
