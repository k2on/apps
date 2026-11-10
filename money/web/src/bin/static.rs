//! money's static site (ui/docs/app.md, "The Browser's Modes"): every page
//! the demo household's screens lead to, prerendered by arkui-web's
//! `prerender` from the same `MoneyWeb` the page runs, each written as
//! `<out>/<route>/index.html` for the hydrating module to take over.
//!
//!   money-web-static <out> [--today 2026-11-10] [--base /] [--cap 250] [--start screen=budget ...] [--shots shots.json]
//!
//! The pages are found from `/` and each `--start` (and each of a
//! shots.json's `screens`, by its launch arguments), breadth first, up to
//! `--cap`: a control leading past them is drawn disabled (the static
//! site's limit, ui/docs/app.md).

use std::path::PathBuf;

use arkui_web::page::{Mode, Route};
use arkui_web::{document, prerender, Written};
use money_web::MoneyWeb;

fn main() -> std::io::Result<()> {
    let mut args = std::env::args().skip(1);
    let out = PathBuf::from(args.next().expect("money-web-static <out> [--today D] [--base /] [--cap N] [--start q]"));
    let (mut today, mut base, mut cap) = (money_core::dates::day_of_ms(ark_peer::now_ms()), "/".to_string(), 250usize);
    let mut starts = vec![Route::default()];
    while let Some(a) = args.next() {
        let v = args.next().unwrap_or_default();
        match a.as_str() {
            "--today" => today = v,
            "--base" => base = v,
            "--cap" => cap = v.parse().expect("a number of pages"),
            "--start" => starts.push(Route::of_query(&v)),
            "--shots" => {
                let shots: serde_json::Value = serde_json::from_slice(&std::fs::read(&v)?).expect("shots.json");
                for s in shots["screens"].as_array().into_iter().flatten() {
                    let a: Vec<String> = s["args"]
                        .as_array()
                        .into_iter()
                        .flatten()
                        .filter_map(|a| a.as_str().map(String::from))
                        .collect();
                    let pairs = a
                        .chunks(2)
                        .filter(|p| p.len() == 2)
                        .map(|p| (p[0].trim_start_matches('-').to_string(), p[1].clone()));
                    starts.push(Route(pairs.collect()));
                }
            }
            other => panic!("money-web-static: what is {other}?"),
        }
    }
    let page_args = vec![("today".to_string(), today.clone())];
    let head = format!(
        "<title>Money</title>\n<link rel=\"icon\" href=\"{base}favicon.svg\" type=\"image/svg+xml\">\n\
         <script type=\"module\">import init from \"{base}pkg/money-web.js?v=dev\"; init({{ module_or_path: \"{base}pkg/money-web_bg.wasm?v=dev\" }});</script>"
    );
    let write = |r: &Route, view: &str| {
        document(
            &Written {
                mode: Mode::Static,
                route: r.clone(),
                base: base.clone(),
                args: page_args.clone(),
                width: None,
                accent: "green".into(),
                css: money_web::CSS.into(),
                head: head.clone(),
                ..Written::default()
            },
            view,
        )
    };
    let open = || MoneyWeb::open(today.clone());
    let pages = prerender(&open, starts, cap, None, &page_args, &write);
    for (r, html) in &pages {
        // On disk as a server finds it: each segment read back.
        let dir = r
            .path()
            .split('/')
            .filter(|s| !s.is_empty())
            .fold(out.clone(), |d, s| d.join(arkui_web::page::decode(s).unwrap_or_else(|| s.into())));
        std::fs::create_dir_all(&dir)?;
        std::fs::write(dir.join("index.html"), html)?;
    }
    eprintln!("money-web-static: {} pages in {}", pages.len(), out.display());
    Ok(())
}
