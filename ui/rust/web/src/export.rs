//! An app's web export (Expo's `expo export --platform web`), the same for
//! every app (ui/docs/app.md, "Export"): what its generated entry point
//! runs natively, `<id>-web <dir>`, beside the module it builds for wasm.
//!
//! An app whose pages are written ahead (`[web] modes` has `ssg`) has each
//! of its paths (`Windowed::paths`) written as HTML by arkui-web's `ssr`
//! side, at every band of width at once, whole without script and taken
//! over by its module (`hydrate`): its `<title>` and description the
//! window's at that path, a canonical link, the app's own `<head>` and
//! files, what its `[web] prepare` worked out carried in every page
//! (`#ark-prepared`, `Session::prepared`), `sitemap.xml` and `robots.txt`;
//! and no link on any page to a page that is not written. An app drawn in
//! the browser alone (`spa`) has one page, the module's.
//!
//!   <id>-web <dir> [--base /apps/docs/] [--site https://k2on.github.io] [--version v]

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use crate::host::{Here, WebHost, WebHosted};
use crate::page::{escape, Mode};
use crate::{document, prerender, Address, Written};

/// How an export is written.
#[derive(Clone, Debug)]
pub struct Options {
    /// Where it is served from: `/`, or `/apps/docs/`.
    pub base: String,
    /// Where it is published, for the sitemap and the canonical links:
    /// `https://k2on.github.io`; none for neither.
    pub site: Option<String>,
    /// The module's `?v=`: the build's own hash, `dev` by hand.
    pub version: String,
    /// The day its pages are worked out at.
    pub today: String,
}

impl Default for Options {
    fn default() -> Self {
        Options {
            base: "/".into(),
            site: None,
            version: "dev".into(),
            today: "2026-01-01".into(),
        }
    }
}

/// The export's command line (`<dir> [--base …] [--site …] [--version …]`),
/// run: what an app's generated entry point's `main` is natively.
pub fn main<W: WebHosted>() {
    let mut args = std::env::args().skip(1);
    let mut out = PathBuf::from("dist");
    let mut o = Options::default();
    while let Some(a) = args.next() {
        match a.as_str() {
            "--base" => o.base = args.next().expect("--base /path/"),
            "--site" => o.site = Some(args.next().expect("--site https://host")),
            "--version" => o.version = args.next().expect("--version v"),
            "--today" => o.today = args.next().expect("--today YYYY-MM-DD"),
            dir => out = PathBuf::from(dir),
        }
    }
    if !o.base.ends_with('/') {
        o.base.push('/');
    }
    match export::<W>(&out, &o) {
        Ok(paths) => println!("{}: wrote {} pages in {}", W::NAME, paths.len(), out.display()),
        Err(e) => {
            eprintln!("{}: the export failed: {e}", W::NAME);
            std::process::exit(1);
        }
    }
}

/// `W`'s web export into `out`: the paths of the pages written.
pub fn export<W: WebHosted>(out: &Path, o: &Options) -> io::Result<Vec<String>> {
    ark_app::set_web();
    fs::create_dir_all(out)?;
    fs::write(out.join("favicon.svg"), W::MARK)?;
    for (name, bytes) in W::FILES {
        let at = out.join(name);
        if let Some(dir) = at.parent() {
            fs::create_dir_all(dir)?;
        }
        fs::write(at, bytes)?;
    }
    if !W::STATIC {
        return Ok(vec![]);
    }
    let prepared = W::prepare(out)?;
    let here = Here {
        today: o.today.clone(),
        base: o.base.clone(),
        prepared: prepared.clone(),
    };
    let open = || WebHost::<W>::over(None, here.clone(), None).with_demo();
    let carried = prepared.replace("</", "<\\/");
    let args = vec![("base".to_string(), o.base.clone())];
    let base = o.base.clone();
    let page = |r: &Address, app: &WebHost<W>, view: &str| {
        let title = app.title();
        let description = app
            .description()
            .map(|d| format!("<meta name=\"description\" content=\"{}\">\n", escape(&d)))
            .unwrap_or_default();
        let canonical = o
            .site
            .as_ref()
            .map(|s| {
                format!(
                    "<link rel=\"canonical\" href=\"{}\">\n",
                    escape(&format!("{s}{}{}", base.trim_end_matches('/'), r.path_url()))
                )
            })
            .unwrap_or_default();
        let head = format!(
            "<title>{title}</title>\n{description}{canonical}<link rel=\"icon\" href=\"{base}favicon.svg\" type=\"image/svg+xml\">\n\
             {own}\n<script type=\"application/json\" id=\"ark-prepared\">{carried}</script>\n\
             <script type=\"module\">import init from \"{base}pkg/{id}-web.js?v={v}\"; init({{ module_or_path: \"{base}pkg/{id}-web_bg.wasm?v={v}\" }});</script>",
            title = escape(&title),
            own = W::HEAD.replace("{base}", &base).replace("{version}", &o.version),
            id = W::ID,
            v = o.version,
        );
        document(
            &Written {
                mode: Mode::Static,
                route: r.clone(),
                base: base.clone(),
                args: args.clone(),
                width: None,
                accent: W::ACCENT.into(),
                css: W::CSS.into(),
                head,
                ..Written::default()
            },
            view,
        )
    };
    let bound = open().paths().len();
    let written = prerender(&open, bound, None, &args, &page).map_err(io::Error::other)?;
    let mut paths = vec![];
    for (r, html) in &written {
        let dir = r.path.iter().fold(out.to_path_buf(), |d, s| d.join(s));
        fs::create_dir_all(&dir)?;
        fs::write(dir.join("index.html"), html)?;
        paths.push(r.path_url());
    }
    // No link on a written page to a page that is not written.
    let urls: Vec<(String, String)> = written
        .iter()
        .map(|(r, h)| (format!("{}{}", base.trim_end_matches('/'), r.path_url()), h.clone()))
        .collect();
    let dangling = crate::page::dangling(&urls, &base);
    if !dangling.is_empty() {
        return Err(io::Error::other(format!("links to no page: {dangling:?}")));
    }
    if let Some(site) = &o.site {
        fs::write(
            out.join("sitemap.xml"),
            format!(
                "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<urlset xmlns=\"http://www.sitemaps.org/schemas/sitemap/0.9\">\n{}</urlset>\n",
                paths
                    .iter()
                    .map(|p| format!("  <url><loc>{site}{}{p}</loc></url>\n", base.trim_end_matches('/')))
                    .collect::<String>()
            ),
        )?;
        fs::write(
            out.join("robots.txt"),
            format!("User-agent: *\nAllow: /\nSitemap: {site}{base}sitemap.xml\n"),
        )?;
    }
    Ok(paths)
}

impl<W: WebHosted> WebHost<W> {
    /// The page over its demo's household when the app has one: what a
    /// written page of an app with data shows, as its screenshots do.
    fn with_demo(self) -> WebHost<W> {
        match W::domain() {
            Some(_) => WebHost::demo(self.here),
            None => self,
        }
    }
}
