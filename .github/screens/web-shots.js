// An app's browser screenshots (app-web.sh): its <app>/src/demo/shots.json
// `wide`, the wide windows' one walk (apps.md, "The wide windows"), taken
// in Playwright's Chromium as the desktop's window and the Mac take it: the
// page as the site publishes its demo (`wide.web.demo`, app-web.sh), on
// DEMO_DAY or `wide.today`, at
// `wide.window` and `wide.scale`, one page a theme, light then dark, each
// shot's steps from where the last left it. A step that names an element
// by its id (`tap`, `text`, `press`, `await`, and `type` into the field
// tapped last) is taken inside the page (`window.arkShots`, arkui-web's
// walk: the views' own, as the desktop's window and the phones' walks);
// the rest are a hand's: `click` a text, `context` a text (its menu),
// `hover`, `key`, `type` with `"into": "keys"` (typed at the keyboard),
// `wait` seconds. A shot or a step `on` other platforms alone is passed
// over; a shot with a `query` (the browser's own: arkui-web's `?context=`,
// `?menu=`, `?popover=`, `?command=`, `?keys=`, and the launch arguments
// every shell reads) is opened in a page of its own at that address, then
// its steps.
//
//   node web-shots.js <shots.json> <base URL> <out>
//
// Writes <out>/<theme>/<id>.png, each with its <id>.tree.txt (what a step
// can name), and <out>/device.json.
const fs = require('fs');
const path = require('path');
const { chromium } = require('playwright');

// The page ready to be taken (ui/docs/shots.md): the app's own signal where
// it has one (`data-ark-idle` = 1: every runtime's `Runtime::is_idle`), at a
// version (`data-ark-version`: the runtimes' `version`) that held for two
// frames; and the page's own: its fonts loaded, every image in sight
// decoded or failed (one out of sight and lazy is not waited for, and one
// that has not come in PICTURE_MS is let go), no animation with an end
// still running. No time is waited for: the limit only fails a page that
// never says so.
async function ready(page) {
  const said = await page.evaluate(() => new Promise(done => {
    const until = performance.now() + 20000;
    let last = null, frames = 0;
    // A picture is waited for only while it could show: one the browser
    // has not asked for (loading="lazy", out of sight) is not, and one
    // asked for that has neither come nor failed within PICTURE_MS is let
    // go — a slow or dead address never holds the page from being ready.
    const PICTURE_MS = 5000;
    const asked = new Map();
    const inSight = i => {
      const r = i.getBoundingClientRect();
      return r.width > 0 && r.height > 0 && r.bottom > 0 && r.right > 0 && r.top < innerHeight && r.left < innerWidth;
    };
    const settled = i => {
      if (i.complete) return true;
      if (i.loading === 'lazy' && !inSight(i)) return true;
      const key = i.currentSrc || i.src;
      if (!asked.has(key)) asked.set(key, performance.now());
      return performance.now() - asked.get(key) > PICTURE_MS;
    };
    const check = async () => {
      await document.fonts.ready;
      const root = document.documentElement;
      const idle = !root.hasAttribute('data-ark-idle') || root.getAttribute('data-ark-idle') === '1';
      const version = root.getAttribute('data-ark-version');
      const moving = document.getAnimations().some(a => a.playState === 'running' && a.effect && a.effect.getComputedTiming().iterations !== Infinity);
      const quiet = idle && !moving && [...document.images].every(settled);
      frames = quiet && version === last ? frames + 1 : 0;
      last = version;
      if (frames >= 2) return done(null);
      if (performance.now() > until) return done(`not ready: idle ${root.getAttribute('data-ark-idle')}, version ${version}, moving ${moving}`);
      requestAnimationFrame(check);
    };
    requestAnimationFrame(check);
  }));
  if (said) throw new Error(said);
}

// Whether a shot or a step is the browser's: one with no `on` is every wide
// window's.
const ours = s => !s.on || s.on.includes('web');

// A step the page's walk takes, as `arkShots.step` is handed it: its kind,
// the id it names (or the text it types) and a menu's item; none for a
// hand's.
function walked(step) {
  if (step.tap) return ['tap', step.tap, ''];
  if (step.text) return ['text', step.text, ''];
  if (step.press && step.item) return ['press', step.press, step.item];
  if (step.await) return ['await', step.await, ''];
  if (step.type !== undefined && step.into !== 'keys') return ['type', step.type, ''];
  return null;
}

// One step: inside the page while what it names is not there yet (asked
// again each time the page is ready anew, for 20 s), else a hand's.
async function take(page, step) {
  const w = walked(step);
  if (w) {
    const until = Date.now() + 20000;
    for (;;) {
      const said = await page.evaluate(([k, id, item]) => window.arkShots.step(k, id, item), w);
      const [what, ...rest] = said.split('\n');
      if (what === 'done') return ready(page);
      if (what === 'failed') throw new Error(`${JSON.stringify(step)}: ${rest.join('\n')}`);
      if (Date.now() > until) throw new Error(`${JSON.stringify(step)} never came; on the page:\n${rest.join('\n')}`);
      await page.waitForTimeout(100);
      await ready(page);
    }
  }
  // `"prefix": true`: a text that begins so (a count after it: "Approve All (5)").
  if (step.click) await page.getByText(step.prefix ? new RegExp('^' + step.click.replace(/[.*+?^${}()|[\]\\]/g, '\\$&')) : step.click, { exact: !step.prefix }).first().click({ button: step.button || 'left' });
  if (step.context) await page.getByText(step.context, { exact: true }).first().click({ button: 'right' });
  if (step.hover) await page.getByText(step.hover, { exact: true }).last().hover();
  if (step.key) await page.keyboard.press(step.key);
  if (step.type !== undefined && step.into === 'keys') await page.keyboard.type(step.type);
  if (step.wait) await page.waitForTimeout(step.wait * 1000);
  await ready(page);
}

async function main() {
  const [shotsFile, base, out] = process.argv.slice(2);
  const all = JSON.parse(fs.readFileSync(shotsFile, 'utf8'));
  const wide = all.wide;
  if (!wide) throw new Error(`${shotsFile} has no "wide"`);
  const [width, height] = wide.window || [1440, 900];
  const scale = wide.scale || 1;
  const today = process.env.DEMO_DAY || wide.today;
  const shots = (wide.shots || []).filter(ours);
  const browser = await chromium.launch({ args: ['--font-render-hinting=none', '--disable-lcd-text', '--force-color-profile=srgb'] });
  let failed = 0;
  for (const theme of ['light', 'dark']) {
    const context = await browser.newContext({ viewport: { width, height }, deviceScaleFactor: scale, colorScheme: theme, reducedMotion: 'reduce' });
    const address = query => {
      // `wide.web.query`: what every address says first, the app's own (the
      // demo is no query: app-web.sh serves the page published as the demo).
      // `shots=1`: nothing on the page moves, a fade included, the caret is
      // clear, and the walk is the page's (`window.arkShots`).
      const q = [(wide.web || {}).query || '', today ? `today=${today}` : '', `theme=${theme}`, 'shots=1', query || ''].filter(Boolean).join('&');
      return `${base}/?${q}`;
    };
    const open = async query => {
      const page = await context.newPage();
      const errors = [];
      page.on('pageerror', e => errors.push(e.message));
      await page.goto(address(query));
      await page.waitForSelector('.ark-window', { timeout: 20000 });
      await page.waitForFunction(() => !!window.arkShots, null, { timeout: 20000 });
      await ready(page);
      return { page, errors };
    };
    // The walk's page, opened once a theme; a shot's own page for one with an address.
    let walk = null;
    const dir = path.join(out, theme);
    fs.mkdirSync(dir, { recursive: true });
    for (const shot of shots) {
      const began = Date.now();
      let at = null;
      try {
        if (shot.query) {
          at = await open(shot.query);
        } else {
          walk = walk || (await open(''));
          at = walk;
        }
        for (const step of (shot.steps || []).filter(ours)) await take(at.page, step);
        await at.page.screenshot({ path: path.join(dir, `${shot.id}.png`) });
        // What a step can name: the walk's ids, as the desktop's tree.txt
        // has them, and the page's text under them.
        const tree = await at.page.evaluate(() => `${window.arkShots.tree()}\n\n${document.body.innerText}`);
        fs.writeFileSync(path.join(dir, `${shot.id}.tree.txt`), tree);
        console.log(`${theme} ${shot.id}: ${((Date.now() - began) / 1000).toFixed(1)}s${at.errors.length ? ' (errors: ' + at.errors.join('; ') + ')' : ''}`);
      } catch (e) {
        failed++;
        console.log(`::error::${theme} ${shot.id}: ${e.message}`);
        if (at) {
          await at.page.screenshot({ path: path.join(dir, 'stuck.png') }).catch(() => {});
          // Wherever the walk stuck is no place for the next: a page of its own.
          if (at === walk) walk = null;
          await at.page.close();
          at = null;
        }
      }
      if (at && at !== walk) await at.page.close();
    }
    if (walk) await walk.page.close();
    await context.close();
  }
  fs.writeFileSync(path.join(out, 'device.json'), JSON.stringify({
    name: `Browser, ${width}×${height}`, width: width * scale, height: height * scale, corner: 0, cutout: 'none',
    browser: 'chromium', version: browser.version(),
  }, null, 2) + '\n');
  await browser.close();
  if (failed) process.exit(1);
}

main().catch(e => {
  console.error(e);
  process.exit(1);
});
