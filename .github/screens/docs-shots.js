// The docs site as a reader sees it: every page in its sitemap, served
// from <dir> under /apps/docs/, at a phone's width and a desktop's, light
// and dark, with script and without, each a screenshot in <out>. With
// script a page fails on any console error or word of hydration; without,
// on a page that does not draw its title.
//
//   NODE_PATH=… node docs-shots.js <dir> <out>
const { chromium } = require("playwright");
const http = require("http");
const fs = require("fs");
const path = require("path");

const [dir, out] = process.argv.slice(2);
const types = { ".html": "text/html", ".js": "text/javascript", ".wasm": "application/wasm", ".css": "text/css", ".svg": "image/svg+xml", ".webp": "image/webp", ".json": "application/json", ".xml": "application/xml" };
const server = http.createServer((req, res) => {
  let p = decodeURIComponent(new URL(req.url, "http://x").pathname).replace(/^\/apps\/docs\//, "/");
  let f = path.join(dir, p);
  if (fs.existsSync(f) && fs.statSync(f).isDirectory()) f = path.join(f, "index.html");
  if (!fs.existsSync(f)) return res.writeHead(404).end();
  res.writeHead(200, { "content-type": types[path.extname(f)] || "application/octet-stream" });
  fs.createReadStream(f).pipe(res);
});

(async () => {
  await new Promise((r) => server.listen(8732, r));
  fs.mkdirSync(out, { recursive: true });
  const sitemap = fs.readFileSync(path.join(dir, "sitemap.xml"), "utf8");
  const pages = [...sitemap.matchAll(/<loc>[^<]*?\/apps\/docs(\/[^<]*)<\/loc>/g)].map((m) => m[1]);
  const browser = await chromium.launch();
  let failed = 0;
  for (const js of [true, false]) {
    for (const width of [390, 1440]) {
      for (const theme of ["light", "dark"]) {
        const ctx = await browser.newContext({ viewport: { width, height: 900 }, colorScheme: theme, javaScriptEnabled: js });
        for (const p of pages) {
          const page = await ctx.newPage();
          const said = [];
          page.on("console", (m) => (m.type() === "error" || /hydrat/i.test(m.text())) && !/manifest|captures|raw\.githubusercontent/.test(m.text()) && said.push(m.text()));
          page.on("pageerror", (e) => said.push(e.message));
          await page.goto(`http://localhost:8732/apps/docs${p}`, { waitUntil: "networkidle" });
          if (js) await page.waitForSelector("html[data-ark-js]", { timeout: 15000 }).catch(() => said.push("the module did not take the page over"));
          const name = `${p.replace(/\//g, "_") || "_"}-${width}-${theme}-${js ? "js" : "nojs"}`;
          await page.screenshot({ path: path.join(out, `${name}.png`) });
          if (!(await page.title())) said.push("no title");
          if (said.length) {
            failed++;
            console.log(`${name}:\n  ${said.join("\n  ")}`);
          }
          await page.close();
        }
        await ctx.close();
      }
    }
  }
  await browser.close();
  server.close();
  console.log(`${pages.length} pages, ${failed} failed`);
  process.exit(failed ? 1 : 0);
})();
