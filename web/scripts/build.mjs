import { copyFileSync, mkdirSync, readFileSync, writeFileSync } from "node:fs";

const dist = new URL("../dist/", import.meta.url);
const assets = new URL("../dist/assets/", import.meta.url);

mkdirSync(assets, { recursive: true });
copyFileSync(new URL("../src/styles.css", import.meta.url), new URL("styles.css", assets));

const html = readFileSync(new URL("../index.html", import.meta.url), "utf8")
  .replace(
    '<script type="module" src="/src/main.ts"></script>',
    '<script type="module" src="/assets/main.js"></script>'
  )
  .replace("</head>", '    <link rel="stylesheet" href="/assets/styles.css" />\n  </head>');

writeFileSync(new URL("index.html", dist), html);
