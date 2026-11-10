//! money in the browser (`money_web`): drawn by the page alone (the SPA;
//! ui/docs/app.md, "The Browser's Modes"), on the server that served it —
//! or, with `?demo`, on the demo's household in the page, seeded to
//! `?today=` (the screenshots', and the public demo's).

use money_core::dates::{day_of_ms, parse_day};
use money_web::MoneyWeb;

fn main() {
    let now = day_of_ms(ark_peer::now_ms());
    let app = match arkui_web::query("demo") {
        Some(_) => MoneyWeb::open(arkui_web::query("today").filter(|d| parse_day(d).is_some()).unwrap_or(now)),
        None => MoneyWeb::on_server(origin(), now),
    };
    arkui_web::run(app, "green", money_web::CSS);
    if let Some(id) = arkui_web::query("popover") {
        arkui_web::after_draw(move || arkui_web::fields::open_popover(&id, 0));
    }
}

/// Whoever served the page: money-server serves it beside its socket and
/// sign-in, so the page's own origin is its server.
fn origin() -> String {
    web_sys::window().and_then(|w| w.location().origin().ok()).unwrap_or_default()
}
