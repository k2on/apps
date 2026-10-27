// What the HTML cannot do alone: remember the theme and the phone, walk the
// pages with j and k, and show each phone's latest capture from the
// `screens` branch (manifest.json, written by .github/screens/manifest.py).
(() => {
  const store = (k, v) => { try { v === undefined ? (v = localStorage.getItem(k)) : localStorage.setItem(k, v); } catch (e) {} return v; };

  // The theme: the system's, or either by hand (the <head> applies it
  // before the first paint).
  const tone = document.getElementById("tone");
  tone.value = store("tone") || "";
  tone.onchange = () => {
    store("tone", tone.value);
    if (tone.value) document.documentElement.dataset.theme = tone.value;
    else delete document.documentElement.dataset.theme;
  };

  // The phone, kept from page to page.
  const phone = document.getElementById(store("phone") || "android");
  if (phone) phone.checked = true;
  document.querySelectorAll("input[name=phone]").forEach((r) => (r.onchange = () => store("phone", r.value)));

  // j and k: the next page and the last, in the sidebar's order.
  addEventListener("keydown", (e) => {
    if (e.metaKey || e.ctrlKey || e.altKey || /INPUT|SELECT|TEXTAREA/.test(e.target.tagName)) return;
    if (e.key !== "j" && e.key !== "k") return;
    const links = [...document.querySelectorAll("nav a")];
    const at = links.findIndex((a) => a.hasAttribute("aria-current"));
    const to = links[at + (e.key === "j" ? 1 : -1)];
    if (to) location.href = to.href;
  });

  // The captures: a still, or a recording as an MP4, in each theme the page
  // can be in, and the device's own shape round them. A recording is fetched
  // only in the theme shown, and plays only while it is on screen.
  const figures = document.querySelectorAll(".capture");
  if (!figures.length) return;
  const base = document.querySelector("meta[name=screens]").content;
  const say = (f, words) => (f.querySelector(".screen p").textContent = words);
  const videos = [];
  const seen = new IntersectionObserver((all) => all.forEach((e) => (e.target.inView = e.isIntersecting)) || play());
  const play = () =>
    videos.forEach((v) => {
      const shown = v.inView && v.offsetParent !== null;
      if (shown && v.paused) {
        v.preload = "auto";
        v.play().catch(() => {});
      } else if (!shown && !v.paused) v.pause();
    });
  tone.addEventListener("change", play);
  document.querySelectorAll("input[name=phone]").forEach((r) => r.addEventListener("change", play));
  matchMedia("(prefers-color-scheme: dark)").addEventListener("change", play);
  fetch(base + "manifest.json", { cache: "no-cache" })
    .then((r) => (r.ok ? r.json() : Promise.reject()))
    .then((m) => {
      figures.forEach((f) => {
        const p = m.platforms[f.dataset.phone];
        const shot = p && p.shots.find((s) => s.id === f.dataset.shot);
        if (p) {
          const d = p.device;
          f.style.setProperty("--aspect", `${d.width} / ${d.height}`);
          f.style.setProperty("--corner", d.corner / d.width);
          f.classList.add(d.cutout);
        }
        if (!shot) return say(f, `Not captured yet${p ? " on " + p.device.name : ""}.`);
        const screen = f.querySelector(".screen");
        const about = f.querySelector("figcaption").textContent;
        for (const theme of ["light", "dark"]) {
          const at = `${base}${f.dataset.phone}/${shot.themes.includes(theme) ? theme : shot.themes[0]}/${shot.id}`;
          let shown;
          if (shot.kind === "recording") {
            shown = document.createElement("video");
            Object.assign(shown, { muted: true, loop: true, playsInline: true, preload: "none", src: `${at}.mp4?v=${m.version}` });
            shown.setAttribute("aria-label", about);
            videos.push(shown);
            seen.observe(shown);
          } else {
            shown = new Image();
            Object.assign(shown, { loading: "lazy", alt: about, src: `${at}.png?v=${m.version}` });
          }
          shown.className = theme;
          screen.append(shown);
        }
        f.querySelector("small").textContent = `${p.device.name} · ${m.commit.slice(0, 7)} · ${m.made}`;
      });
    })
    .catch(() => figures.forEach((f) => say(f, "No captures published yet: the screens workflow has not run.")));
})();
