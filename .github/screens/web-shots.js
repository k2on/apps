// An app's browser screenshots (app-web.sh): its <app>/screens/shots.json
// `screens` (each by its launch arguments, the address's parameters here)
// and `web`, each shot opened by its address (`query`: the launch arguments
// every shell reads, `?screen=…`, and arkui-web's `?context=`, `?menu=`,
// `?popover=`, `?command=`, `?keys=`) and then its steps — the desktop's
// vocabulary: `click` a text on the page, `key` a key, `type` text,
// `await` a text, `wait` seconds — at each size in light and in dark, in
// Playwright's Chromium: one page for each size and theme, every shot's
// address taken in it in turn (a `fresh` shot, a page of its own).
//
//   node web-shots.js <shots.json> <base URL> <out>
//
// Writes <out>/<size>/<theme>/<id>.png and <out>/device.json.
//
// The page's mode (ui/docs/app.md, "The Browser's Modes") is ARK_WEB_MODE:
// `spa` (the default) as above; `ssg`, a static site, where each of
// `screens` is opened by its route's address (`/budget/?inspector=1`) and the
// browser's own `web.shots` (which open the page's transient parts by its
// query) are left out. ARK_WEB_JS=0 takes them with script off. With
// script on, a page written as HTML fails its shot on any error or any
// word of hydration in the console: the browser's first draw must be the
// one the page was written with.
const fs = require('fs');
const path = require('path');
const { chromium } = require('playwright');

// The page ready to be taken (ui/docs/shots.md): the app's own signal where
// it has one (`data-ark-idle` = 1: every runtime's `Runtime::is_idle`), at a
// version (`data-ark-version`: the runtimes' `version`) that held for two
// frames; and the page's own: its fonts loaded, every image decoded, no
// animation with an end still running. No time is waited for: the limit
// only fails a page that never says so.
async function ready(page) {
  const said = await page.evaluate(() => new Promise(done => {
    const until = performance.now() + 20000;
    let last = null, frames = 0;
    const check = async () => {
      await document.fonts.ready;
      const root = document.documentElement;
      const idle = !root.hasAttribute('data-ark-idle') || root.getAttribute('data-ark-idle') === '1';
      const version = root.getAttribute('data-ark-version');
      const moving = document.getAnimations().some(a => a.playState === 'running' && a.effect && a.effect.getComputedTiming().iterations !== Infinity);
      const quiet = idle && !moving && [...document.images].every(i => i.complete);
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
  const mode = process.env.ARK_WEB_MODE || 'spa';
  const js = process.env.ARK_WEB_JS !== '0';
  // A route's address, as arkui-web's `page::Route::url` writes it: the
  // screen the path, the rest the query (the UI's state, which a static
  // page takes once its script runs).
  const routePath = args => {
    const q = [];
    let path = '/';
    for (let i = 0; i + 1 < args.length; i += 2) {
      const k = args[i].replace(/^-+/, ''), v = encodeURIComponent(args[i + 1]).replace(/%3A/g, ':');
      if (k === 'screen') path = `/${v}/`; else q.push(`${k}=${v}`);
    }
    return q.length ? `${path}?${q.join('&')}` : path;
  };
  const screens = (all.screens || []).map(s => ({ id: s.id, query: query(s.args), path: routePath(s.args), sizes: s.on }));
  const shots = mode === 'spa' ? screens.concat(web.shots || []) : screens;
  const browser = await chromium.launch({ args: ['--font-render-hinting=none', '--disable-lcd-text', '--force-color-profile=srgb'] });
  let failed = 0;
  for (const [size, [width, height]] of Object.entries(sizes)) {
    for (const theme of ['light', 'dark']) {
      const context = await browser.newContext({ viewport: { width, height }, deviceScaleFactor: web.scale || 1, colorScheme: theme, reducedMotion: 'reduce', javaScriptEnabled: js });
      // One page for every shot at this size and theme (the SPA's): each
      // shot's address taken in place (`history.pushState` and its
      // `popstate`, which arkui-web answers by putting the app back where it
      // opens, its menus and popovers closed, and bringing it where the
      // address says); a page of its own only for a shot that says it needs
      // a clean start (`fresh`), and for every page of a static site.
      let page = null;
      let errors = [];
      const open = async () => {
        if (page) await page.close();
        page = await context.newPage();
        page.on('pageerror', e => errors.push(e.message));
        page.on('console', m => {
          if (mode !== 'spa' && (m.type() === 'error' || /hydrat/i.test(m.text()))) errors.push(`console ${m.type()}: ${m.text()}`);
        });
      };
      for (const shot of shots) {
        if (shot.sizes && !shot.sizes.includes(size)) continue;
        errors = [];
        const began = Date.now();
        const inPlace = mode === 'spa' && page && !shot.fresh;
        if (!inPlace) await open();
        // `web.query`: what every shot's address says first, the app's own
        // (money's `demo`: the household in the page, no server).
        // `shots=1`: nothing on the page moves, a fade included, and the
        // caret is clear (arkui-web's `data-motion="shots"`, ui/docs/shots.md).
        const q = [web.query || '', `today=${web.today}`, `theme=${theme}`, 'shots=1', shot.query || ''].filter(Boolean).join('&');
        // How a shot's `route` is put in the address: the one place that says
        // it (a router's URL is its fragment, as the address bar shows it).
        const at = route => (route ? `#${route.replace(/^[a-z]+:\/\//, '')}` : '');
        try {
          const url = mode === 'spa' ? `${base}/?${q}${at(shot.route)}` : `${base}${shot.path}`;
          if (inPlace) {
            await page.evaluate(u => {
              history.pushState(null, '', u);
              window.dispatchEvent(new PopStateEvent('popstate'));
            }, url);
            // An app that cannot put itself back loads the address afresh.
            await page.waitForLoadState('load');
          } else {
            await page.goto(url);
          }
          // The written page is the browser's once its module has run.
          if (mode !== 'spa' && js) await page.waitForSelector('html[data-ark-js]', { timeout: 20000 });
          await page.waitForSelector('.ark-window', { timeout: 20000 });
          await ready(page);
          for (const step of shot.steps || []) {
            if (step.click) await page.getByText(step.click, { exact: true }).first().click({ button: step.button || 'left' });
            if (step.context) await page.getByText(step.context, { exact: true }).first().click({ button: 'right' });
            if (step.hover) await page.getByText(step.hover, { exact: true }).last().hover();
            if (step.key) await page.keyboard.press(step.key);
            if (step.type) await page.keyboard.type(step.type);
            if (step.await) await page.getByText(step.await).first().waitFor({ timeout: 10000 });
            if (step.wait) await page.waitForTimeout(step.wait * 1000);
            await ready(page);
          }
          const dir = path.join(out, size, theme);
          fs.mkdirSync(dir, { recursive: true });
          await page.screenshot({ path: path.join(dir, `${shot.id}.png`) });
          // The texts on the page, as the desktop's tree.txt has them: what a step can name.
          fs.writeFileSync(path.join(dir, `${shot.id}.tree.txt`), await page.evaluate(() => document.body.innerText));
          console.log(`${size} ${theme} ${shot.id}: ${inPlace ? 'in place' : 'loaded'}, ${((Date.now() - began) / 1000).toFixed(1)}s${errors.length ? ' (errors: ' + errors.join('; ') + ')' : ''}`);
          if (mode !== 'spa' && errors.length) {
            failed++;
            console.log(`::error::${size} ${theme} ${shot.id}: ${errors.join('; ')}`);
          }
        } catch (e) {
          failed++;
          console.log(`::error::${size} ${theme} ${shot.id}: ${e.message}`);
          // Wherever it stuck is no place for the next: a page of its own.
          await page.close();
          page = null;
        }
      }
      if (page) await page.close();
      await context.close();
    }
  }
  fs.writeFileSync(path.join(out, 'device.json'), JSON.stringify({ browser: 'chromium', version: browser.version(), sizes, mode, js }, null, 2) + '\n');
  await browser.close();
  if (failed) process.exit(1);
}

main().catch(e => {
  console.error(e);
  process.exit(1);
});
