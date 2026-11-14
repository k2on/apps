// What the page's module does not: each phone's latest capture from the
// `screens` branch (manifest.json, written by .github/screens/manifest.py),
// a still or an MP4 in each theme, in the device's own shape. A capture
// drawn again by the module is filled again; a recording plays only while
// it is on screen. The appearance the reader chose is the <head>'s.
(() => {
  const base = document.querySelector("meta[name=screens]").content;
  let manifest;
  const videos = [];
  const seen = new IntersectionObserver((all) => all.forEach((e) => (e.target.inView = e.isIntersecting)) || play());
  const play = () =>
    videos.forEach((v) => {
      const shown = v.isConnected && v.inView && v.offsetParent !== null;
      if (shown && v.paused) {
        v.preload = "auto";
        v.play().catch(() => {});
      } else if (!shown && !v.paused) v.pause();
    });
  const say = (f, words) => {
    const p = f.querySelector(".screen .ark-js");
    if (p) p.textContent = words;
  };
  const fill = (f) => {
    const screen = f.querySelector(".screen");
    if (!screen || screen.querySelector("img, video") || f.dataset.said) return;
    if (manifest === null) return say(f, "No captures published yet: the screens workflow has not run.");
    if (!manifest) return;
    const p = manifest.platforms[f.dataset.phone];
    const shot = p && p.shots.find((s) => s.id === f.dataset.shot);
    if (p) {
      const d = p.device;
      f.style.setProperty("--aspect", `${d.width} / ${d.height}`);
      f.style.setProperty("--corner", d.corner / d.width);
      f.classList.add(d.cutout);
    }
    if (!shot) {
      f.dataset.said = "1";
      return say(f, `Not captured yet${p ? " on " + p.device.name : ""}.`);
    }
    const about = f.querySelector("figcaption").textContent;
    for (const theme of ["light", "dark"]) {
      const at = `${base}${f.dataset.phone}/${shot.themes.includes(theme) ? theme : shot.themes[0]}/${shot.id}`;
      let shown;
      if (shot.kind === "recording") {
        shown = document.createElement("video");
        Object.assign(shown, { muted: true, loop: true, playsInline: true, preload: "none", src: `${at}.mp4?v=${manifest.version}` });
        shown.setAttribute("aria-label", about);
        videos.push(shown);
        seen.observe(shown);
      } else {
        shown = new Image();
        Object.assign(shown, { loading: "lazy", alt: about, src: `${at}.png?v=${manifest.version}` });
      }
      shown.className = theme;
      screen.append(shown);
    }
    f.querySelector("small").textContent = `${p.device.name} · ${manifest.commit.slice(0, 7)} · ${manifest.made}`;
  };
  const fillAll = () => document.querySelectorAll(".capture").forEach(fill);
  // The module draws a page again on every message: fill what is new.
  new MutationObserver(() => {
    fillAll();
    play();
  }).observe(document.body, { childList: true, subtree: true });
  matchMedia("(prefers-color-scheme: dark)").addEventListener("change", play);
  new MutationObserver(play).observe(document.documentElement, { attributes: true, attributeFilter: ["data-theme"] });
  if (!document.querySelector(".capture")) {
    // Fetched once a page with captures is shown.
    const wait = new MutationObserver(() => document.querySelector(".capture") && (wait.disconnect(), load()));
    wait.observe(document.body, { childList: true, subtree: true });
  } else load();
  function load() {
    fetch(base + "manifest.json", { cache: "no-cache" })
      .then((r) => (r.ok ? r.json() : Promise.reject()))
      .then((m) => (manifest = m))
      .catch(() => (manifest = null))
      .finally(fillAll);
  }
})();
