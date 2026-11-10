//! money in the browser (`money_web`): drawn by the page alone (the SPA,
//! feature `csr`), or taking over the HTML the prerenderer or the server
//! wrote (`hydrate`; ui/docs/app.md, "The Browser's Modes").

use money_core::dates::{day_of_ms, parse_day};
use money_web::MoneyWeb;

fn main() {
    let today = arkui_web::query("today")
        .filter(|d| parse_day(d).is_some())
        .unwrap_or_else(|| day_of_ms(ark_peer::now_ms()));
    let app = MoneyWeb::open(today);
    #[cfg(feature = "hydrate")]
    arkui_web::hydrate(app);
    #[cfg(not(feature = "hydrate"))]
    {
        arkui_web::run(app, "green", money_web::CSS);
        if let Some(id) = arkui_web::query("popover") {
            arkui_web::after_draw(move || arkui_web::fields::open_popover(&id, 0));
        }
    }
}
