//! The docs' module (`docs-web`, feature `hydrate`): it takes the written
//! page over (`arkui_web::hydrate`) from what the native build worked out
//! (`#docs-built`), then takes up what is the reader's — the search in the
//! address, the phone chosen — and keeps the address in step as pages are
//! walked: a link to a page of the site is the app's, back and forward are
//! too.
use docs::app::{browser, DocsWeb, Msg};
use docs::pages::{self, Built};
use wasm_bindgen::closure::Closure;
use wasm_bindgen::JsCast;

/// The page's tag at `path` under `base`: its route's (`docs::routes`).
fn tag_at(path: &str, base: &str) -> String {
    docs::routes::Route::from_address(&arkui_web::Address::of_url(path, base))
        .map(|r| r.tag().to_string())
        .unwrap_or_else(|| pages::OVERVIEW.to_string())
}

fn main() {
    let window = web_sys::window().expect("a window");
    let doc = window.document().expect("a document");
    let built: Built = doc
        .get_element_by_id("docs-built")
        .and_then(|e| e.text_content())
        .and_then(|t| serde_json::from_str(&t).ok())
        .unwrap_or_default();
    let base = arkui_web::page_base();
    let app = DocsWeb::open(&base, built);
    let known: Vec<String> = app.pages().iter().map(|p| p.tag.clone()).collect();
    arkui_web::hydrate(app);

    // The reader's, after the written page is taken over.
    if let Some(phone) = browser::kept("phone") {
        arkui_web::send(Msg::Phone { phone });
    }
    // The search in the address (`?q=`) is taken by `hydrate` once the
    // written page is the module's (`WebApp::arrive`).

    // Back and forward.
    {
        let base = base.clone();
        let pop = Closure::<dyn FnMut(web_sys::Event)>::new(move |_| {
            let path = web_sys::window().and_then(|w| w.location().pathname().ok()).unwrap_or_default();
            arkui_web::send(Msg::Go { tag: tag_at(&path, &base) });
        });
        let _ = window.add_event_listener_with_callback("popstate", pop.as_ref().unchecked_ref());
        pop.forget();
    }

    // A link to one of the site's pages walks there in the app.
    let click = Closure::<dyn FnMut(web_sys::MouseEvent)>::new(move |e: web_sys::MouseEvent| {
        if e.default_prevented() || e.button() != 0 || e.meta_key() || e.ctrl_key() || e.shift_key() || e.alt_key() {
            return;
        }
        let Some(a) = e
            .target()
            .and_then(|t| t.dyn_into::<web_sys::Element>().ok())
            .and_then(|el| el.closest("a[href]").ok().flatten())
        else {
            return;
        };
        let href = a.get_attribute("href").unwrap_or_default();
        if a.has_attribute("target") || !href.starts_with(&base) || href.contains('#') || href.contains('?') {
            return;
        }
        let tag = tag_at(&href, &base);
        if known.contains(&tag) {
            e.prevent_default();
            arkui_web::send(Msg::Go { tag });
        }
    });
    let _ = doc.add_event_listener_with_callback("click", click.as_ref().unchecked_ref());
    click.forget();
}
