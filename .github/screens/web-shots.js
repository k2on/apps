// An app's browser screenshots (app-web.sh): its <app>/screens/shots.json
// `screens` (each by its launch arguments, the address's parameters here)
// and `web`, each shot opened by its address (`query`: the launch arguments
// every shell reads, `?screen=…`, and arkui-web's `?context=`, `?menu=`,
// `?popover=`, `?command=`, `?keys=`) and then its steps — the desktop's
// vocabulary: `click` a text on the page, `key` a key, `type` text,
// `await` a text, `wait` seconds — at each size in light and in dark, in
// Playwright's Chromium.
//
//   node web-shots.js <shots.json> <base URL> <out>
//
// Writes <out>/<size>/<theme>/<id>.png and <out>/device.json.
const fs = require('fs');
const path = require('path');
const { chromium } = require('playwright');

async function main() {
  const [shotsFile, base, out] = process.argv.slice(2);
  const all = JSON.parse(fs.readFileSync(shotsFile, 'utf8'));
  const web = all.web;
  if (!web) throw new Error(`${shotsFile} has no "web"`);
  const sizes = web.sizes;
  // Every app's screens by their launch arguments (`-screen budget
  // -inspector 1`), the address's parameters here, on the sizes each is on;
  // then the browser's own.
  const query = args => {
    const q = [];
    for (let i = 0; i + 1 < args.length; i += 2) q.push(`${args[i].replace(/^-+/, '')}=${encodeURIComponent(args[i + 1])}`);
    return q.join('&');
  };
  const shots = (all.screens || []).map(s => ({ id: s.id, query: query(s.args), sizes: s.on })).concat(web.shots || []);
  const browser = await chromium.launch({ args: ['--font-render-hinting=none', '--disable-lcd-text', '--force-color-profile=srgb'] });
  let failed = 0;
  for (const [size, [width, height]] of Object.entries(sizes)) {
    for (const theme of ['light', 'dark']) {
      const context = await browser.newContext({ viewport: { width, height }, deviceScaleFactor: web.scale || 1, colorScheme: theme, reducedMotion: 'reduce' });
      for (const shot of shots) {
        if (shot.sizes && !shot.sizes.includes(size)) continue;
        const page = await context.newPage();
        const errors = [];
        page.on('pageerror', e => errors.push(e.message));
        // `shots=1`: nothing on the page moves, and it says when it is ready
        // (arkui-web's SHOTS_JS, ui/docs/shots.md). Each step is done, then the
        // page's next ready moment after it awaited: no time waited for
        // anywhere; the timeouts are only how long a page may fail to say so.
        const q = [`today=${web.today}`, `theme=${theme}`, 'shots=1', shot.query || ''].filter(Boolean).join('&');
        // How a shot's `route` is put in the address: the one place that says
        // it (a router's URL is its fragment, as the address bar shows it).
        const at = route => (route ? `#${route.replace(/^[a-z]+:\/\//, '')}` : '');
        const generation = () => page.evaluate(() => window.__ark.generation);
        const settled = g => page.evaluate(g => window.__ark.after(g), g);
        try {
          await page.goto(`${base}/?${q}${at(shot.route)}`);
          await page.waitForFunction(() => window.__ark, null, { timeout: 20000 });
          await settled(0);
          for (const step of shot.steps || []) {
            const g = await generation();
            if (step.click) await page.getByText(step.click, { exact: true }).first().click({ button: step.button || 'left' });
            if (step.context) await page.getByText(step.context, { exact: true }).first().click({ button: 'right' });
            if (step.hover) await page.getByText(step.hover, { exact: true }).last().hover();
            if (step.key) await page.keyboard.press(step.key);
            if (step.type) await page.keyboard.type(step.type);
            if (step.await) await page.getByText(step.await).first().waitFor({ timeout: 10000 });
            await settled(g);
          }
          const dir = path.join(out, size, theme);
          fs.mkdirSync(dir, { recursive: true });
          await page.screenshot({ path: path.join(dir, `${shot.id}.png`) });
          // The texts on the page, as the desktop's tree.txt has them: what a step can name.
          fs.writeFileSync(path.join(dir, `${shot.id}.tree.txt`), await page.evaluate(() => document.body.innerText));
          console.log(`${size} ${theme} ${shot.id}${errors.length ? ' (errors: ' + errors.join('; ') + ')' : ''}`);
        } catch (e) {
          failed++;
          console.log(`::error::${size} ${theme} ${shot.id}: ${e.message}`);
        }
        await page.close();
      }
      await context.close();
    }
  }
  fs.writeFileSync(path.join(out, 'device.json'), JSON.stringify({ browser: 'chromium', version: browser.version(), sizes }, null, 2) + '\n');
  await browser.close();
  if (failed) process.exit(1);
}

main().catch(e => {
  console.error(e);
  process.exit(1);
});
