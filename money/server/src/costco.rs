//! Costco's receipts, read in a browser people sign in to themselves (the
//! server's `MONEY_BROWSER`, [`crate::browser`]): costco.com's sign-in is
//! behind bot protection that only a real browser, with a person at it,
//! passes, and its token is good only for the page that holds it.
//!
//! - **Signing in**: Settings has the browser open costco.com's sign-in
//!   ([`Costco::show_sign_in`]), and its page (`MONEY_BROWSER_PAGE`) is
//!   where a person signs in. Costco keeps that browser signed in; nothing
//!   of the sign-in comes to the server.
//! - **Each sync** ([`Costco::receipts`]): a tab of the server's own opens
//!   costco.com's Orders & Purchases, which signs itself in from the
//!   browser's session and keeps its token in the page (`idToken`); once
//!   it has, the page asks Costco's GraphQL for the warehouse and gas
//!   receipts of the last `MONEY_COSTCO_DAYS`, item by item, as its own
//!   list does but with every line's amount, for
//!   `money_core::stores::costco`. A browser not signed in is said so.
//!
//! None of it is a published API: the page's address, the query and its
//! headers are what costco.com's own pages were seen to use.

use std::time::Duration;

use anyhow::{anyhow, bail, Result};
use serde_json::Value as Json;

use crate::browser::Browser;

/// Where a person signs in to costco.com.
pub const SIGN_IN: &str = "https://www.costco.com/LogonForm";

/// costco.com's Orders & Purchases, whose page holds the token its own
/// requests carry.
pub const ORDERS: &str = "https://www.costco.com/myaccount/#/app/4900eb1f-0c10-4bd9-99c3-c59e6c1ecebf/ordersandpurchases";

const RECEIPTS_URL: &str = "https://ecom-api.costco.com/ebusiness/order/v1/orders/graphql";

/// How long the page may take to sign itself in.
const SIGNING_IN: Duration = Duration::from_secs(60);

/// How many days a request for receipts covers: costco.com's own list asks
/// for three months at a time.
pub const WINDOW: i64 = 90;

/// `since` to `today` in windows of [`WINDOW`] days, first to last.
fn windows(since: chrono::NaiveDate, today: chrono::NaiveDate) -> Vec<(chrono::NaiveDate, chrono::NaiveDate)> {
    let mut out = vec![];
    let mut start = since;
    while start <= today {
        let end = (start + chrono::Duration::days(WINDOW - 1)).min(today);
        out.push((start, end));
        start = end + chrono::Duration::days(1);
    }
    out
}

fn parse_day(s: &str) -> Result<chrono::NaiveDate> {
    chrono::NaiveDate::parse_from_str(s, "%Y-%m-%d").map_err(|e| anyhow!("{s} is not a day: {e}"))
}

/// The first pull's marker, in the data directory: the browser's sign-in is
/// not the server's to see, so it names no account, and signing in again
/// from Settings ([`forget_pulled`]) pulls again.
fn pulled_file(data: &std::path::Path) -> std::path::PathBuf {
    data.join("costco-pulled")
}

/// Whether the first pull is done.
pub fn pulled(data: &std::path::Path) -> bool {
    pulled_file(data).exists()
}

pub fn mark_pulled(data: &std::path::Path) -> Result<()> {
    std::fs::write(pulled_file(data), "").map_err(|e| anyhow!("marking Costco's first pull: {e}"))
}

pub fn forget_pulled(data: &std::path::Path) {
    let _ = std::fs::remove_file(pulled_file(data));
}

/// Costco's receipts, through the browser, and how far back.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Costco {
    pub browser: Option<Browser>,
    /// Where people see the browser and sign in, for what is said.
    pub page: Option<String>,
    /// How far back receipts are asked for.
    pub days: u32,
}

impl Default for Costco {
    fn default() -> Self {
        Costco {
            browser: None,
            page: None,
            days: 90,
        }
    }
}

/// The GraphQL query Costco's web app lists receipts with, but asking for
/// each item's lines as its single receipt's page does: the list's own
/// asks for item numbers alone, and the adapter needs every line's amount.
const QUERY: &str = r#"query receiptsWithCounts($startDate: String!, $endDate: String!,$documentType:String!,$documentSubType:String!) {
    receiptsWithCounts(startDate: $startDate, endDate: $endDate,documentType:$documentType,documentSubType:$documentSubType) {
    inWarehouse
    gasStation
    carWash
    gasAndCarWash
    receipts{
    warehouseName warehouseShortName receiptType documentType transactionDateTime transactionDate transactionBarcode transactionType total
    totalItemCount
    itemArray {
      itemNumber
      itemDescription01
      itemDescription02
      itemIdentifier
      unit
      amount
      fuelUnitQuantity
      itemUnitPriceAmount
    }
    tenderArray {
      tenderTypeCode
      tenderDescription
      amountTender
    }
    couponArray {
      upcnumberCoupon
    }
  }
}
  }"#;

/// Where the page is: signed out (it went to the sign-in), or holding its
/// token; null while it is neither yet.
const WHERE: &str = r#"(() => {
  if (location.hostname.startsWith("signin.") || /LogonForm|Logon/i.test(location.href)) return "signed out";
  try { if (localStorage.getItem("idToken")) return "signed in"; } catch (e) {}
  return null;
})()"#;

impl Costco {
    fn browser(&self) -> Result<&Browser> {
        self.browser
            .as_ref()
            .ok_or_else(|| anyhow!("this server has no browser for Costco (services.money.browser)"))
    }

    /// Costco's sign-in opened in the browser, for a person to sign in on
    /// its page.
    pub fn show_sign_in(&self) -> Result<()> {
        self.browser()?.show(SIGN_IN)
    }

    /// What signing in again is said with.
    fn sign_in_again(&self) -> String {
        match &self.page {
            Some(p) => format!("sign in to costco.com in the browser at {p} (Settings \u{2192} Costco)"),
            None => "sign in to costco.com in the server's browser (Settings \u{2192} Costco)".into(),
        }
    }

    /// The receipts of the last `days`, or, for the first pull, since the
    /// day `since` — each window of [`WINDOW`] days asked on its own, as
    /// costco.com's own list asks them — as the adapter reads them; none
    /// when the server has no browser.
    pub fn receipts(&self, since: Option<&str>) -> Result<Option<String>> {
        let Some(browser) = &self.browser else { return Ok(None) };
        let today = chrono::Utc::now().date_naive();
        let windows = match since {
            Some(day) => windows(parse_day(day)?, today),
            None => vec![(today - chrono::Duration::days(self.days as i64), today)],
        };
        let mut tab = browser.open(ORDERS)?;
        match tab.until(WHERE, SIGNING_IN) {
            Ok(w) if w == "signed in" => {}
            Ok(_) => bail!("Costco is signed out: {}", self.sign_in_again()),
            Err(e) => bail!("costco.com's Orders & Purchases did not sign itself in ({e:#}): {}", self.sign_in_again()),
        }
        let mut all = vec![];
        for (start, end) in windows {
            let data = self.window(&mut tab, start, end)?;
            all.extend(data["receiptsWithCounts"]["receipts"].as_array().cloned().unwrap_or_default());
        }
        Ok(Some(serde_json::json!({ "receiptsWithCounts": { "receipts": all } }).to_string()))
    }

    /// One window's receipts, as Costco answered (its `data`).
    fn window(&self, tab: &mut crate::browser::Tab, start: chrono::NaiveDate, end: chrono::NaiveDate) -> Result<Json> {
        // The page refreshes a token it kept when it opens; one asked
        // with the old one is asked again once it has.
        for attempt in 0..3 {
            let said = tab.eval(&self.ask(start, end))?;
            let status = said["status"].as_u64().unwrap_or(0);
            if status == 401 || status == 403 {
                if attempt < 2 {
                    std::thread::sleep(Duration::from_secs(5));
                }
                continue;
            }
            if status != 200 {
                bail!("Costco's receipts: {status}");
            }
            let body: Json = serde_json::from_str(said["body"].as_str().unwrap_or("")).map_err(|e| anyhow!("Costco's receipts are not JSON: {e}"))?;
            if !body["data"]["receiptsWithCounts"].is_object() {
                bail!("Costco did not return receipts");
            }
            return Ok(body["data"].clone());
        }
        bail!("Costco refused the page's sign-in: {}", self.sign_in_again())
    }

    /// The page's own request for the receipts from `start` to `end`, with
    /// its token: what it answered, as `{status, body}`.
    fn ask(&self, start: chrono::NaiveDate, end: chrono::NaiveDate) -> String {
        let variables = serde_json::json!({
            "startDate": start.format("%-m/%-d/%Y").to_string(),
            "endDate": end.format("%-m/%-d/%Y").to_string(),
            "documentType": "all",
            "documentSubType": "all",
        });
        let body = serde_json::json!({ "query": QUERY, "variables": variables }).to_string();
        format!(
            r#"(async () => {{
  const token = localStorage.getItem("idToken");
  const r = await fetch({url}, {{
    method: "POST",
    headers: {{
      "Content-Type": "application/json-patch+json",
      "costco.env": "PROD",
      "costco.service": "restOrders",
      "costco-x-authorization": "Bearer " + token,
      "costco-x-wcs-clientId": "4900eb1f-0c10-4bd9-99c3-c59e6c1ecebf",
      "client-identifier": "481b1aec-aa3b-454b-b81b-48187e28f205",
    }},
    body: {body},
  }});
  return {{ status: r.status, body: await r.text() }};
}})()"#,
            url = serde_json::to_string(RECEIPTS_URL).unwrap(),
            body = serde_json::to_string(&body).unwrap(),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::browser::fake;
    use serde_json::json;

    const RECEIPTS: &str = r#"{"data":{"receiptsWithCounts":{"inWarehouse":1,"gasStation":0,"carWash":0,"gasAndCarWash":0,"receipts":[
        {"warehouseName":"SEATTLE","receiptType":"In-Warehouse","documentType":"WarehouseReceiptDetail",
         "transactionDateTime":"2026-10-25T14:33:00","transactionBarcode":"21134300501862310251433","total":10.83,
         "itemArray":[{"itemNumber":"1234","itemDescription01":"KS ORGANIC EGGS","itemDescription02":"24 CT","unit":1,"amount":10.83}]}
    ]}}}"#;

    fn at(browser: Browser) -> Costco {
        Costco {
            browser: Some(browser),
            page: Some("https://sivrad.example.com/browser".into()),
            days: 30,
        }
    }

    #[test]
    fn no_browser_reads_nothing() {
        assert_eq!(Costco::default().receipts(None).unwrap(), None);
    }

    /// The page signs itself in, then asks for the receipts with its own
    /// token; what it is given is what the domain's adapter reads.
    #[test]
    fn the_page_asks_for_the_receipts_once_it_is_signed_in() {
        let asked = std::sync::atomic::AtomicUsize::new(0);
        let (browser, seen) = fake::serve(move |script| {
            if script.contains("location.hostname") {
                // Still signing itself in, then signed in.
                match asked.fetch_add(1, std::sync::atomic::Ordering::SeqCst) {
                    0 => Json::Null,
                    _ => json!("signed in"),
                }
            } else {
                json!({ "status": 200, "body": RECEIPTS })
            }
        });
        let out = at(browser).receipts(None).unwrap().unwrap();
        std::thread::sleep(Duration::from_millis(200));
        let seen = seen.lock().unwrap();
        assert_eq!(seen.opened, [ORDERS]);
        let ask = seen.scripts.last().unwrap();
        assert!(
            ask.contains(r#"localStorage.getItem("idToken")"#) && ask.contains("ecom-api.costco.com"),
            "{ask}"
        );
        assert!(
            ask.contains("itemDescription01") && ask.contains("amount"),
            "the items' lines are asked for"
        );
        assert!(ask.contains(r#""costco.service": "restOrders""#), "{ask}");
        assert_eq!(seen.closed, 1, "its tab closed");

        let import = money_core::stores::costco(&out).unwrap();
        let r = &import.receipts[0];
        assert_eq!((r.date.as_str(), r.place.as_str(), r.total), ("2026-10-25", "Costco SEATTLE", -1083));
        assert_eq!(r.items[0].name, "KS ORGANIC EGGS 24 CT");
    }

    #[test]
    fn a_browser_signed_out_is_said_with_where_to_sign_in() {
        let (browser, _seen) = fake::serve(|_| json!("signed out"));
        let e = at(browser).receipts(None).unwrap_err().to_string();
        assert!(e.contains("signed out") && e.contains("https://sivrad.example.com/browser"), "{e}");
    }

    #[test]
    fn a_token_refused_is_asked_again_then_said() {
        let (browser, seen) = fake::serve(|script| match script.contains("location.hostname") {
            true => json!("signed in"),
            false => json!({ "status": 401, "body": "" }),
        });
        let e = at(browser).receipts(None).unwrap_err().to_string();
        assert!(e.contains("refused the page's sign-in"), "{e}");
        let asks = seen.lock().unwrap().scripts.iter().filter(|s| s.contains("fetch(")).count();
        assert_eq!(asks, 3);
    }

    #[test]
    fn the_sign_in_is_shown_and_left_open() {
        let (browser, seen) = fake::serve(|_| Json::Null);
        at(browser).show_sign_in().unwrap();
        std::thread::sleep(Duration::from_millis(200));
        let seen = seen.lock().unwrap();
        assert_eq!((seen.opened.as_slice(), seen.closed), ([SIGN_IN.to_string()].as_slice(), 0));
    }

    #[test]
    fn the_first_pull_asks_window_by_window_back_to_its_day() {
        let d = |s: &str| parse_day(s).unwrap();
        let w = windows(d("2026-01-01"), d("2026-07-15"));
        assert_eq!(
            w,
            [
                (d("2026-01-01"), d("2026-03-31")),
                (d("2026-04-01"), d("2026-06-29")),
                (d("2026-06-30"), d("2026-07-15")),
            ],
            "90 days each, touching, the last to today"
        );
        assert_eq!(windows(d("2026-07-15"), d("2026-07-15")), [(d("2026-07-15"), d("2026-07-15"))]);

        // Each window asked of the page, and their receipts one import.
        let (browser, seen) = fake::serve(|script| match script.contains("location.hostname") {
            true => json!("signed in"),
            false => json!({ "status": 200, "body": RECEIPTS }),
        });
        let since = (chrono::Utc::now().date_naive() - chrono::Duration::days(200))
            .format("%Y-%m-%d")
            .to_string();
        let out = at(browser).receipts(Some(&since)).unwrap().unwrap();
        let asks = seen.lock().unwrap().scripts.iter().filter(|s| s.contains("fetch(")).count();
        assert_eq!(asks, 3, "201 days: three windows");
        let import = money_core::stores::costco(&out).unwrap();
        assert_eq!(import.receipts.len(), 3, "a receipt from each window's answer");

        let dir = tempfile::tempdir().unwrap();
        assert!(!pulled(dir.path()));
        mark_pulled(dir.path()).unwrap();
        assert!(pulled(dir.path()));
        forget_pulled(dir.path());
        assert!(!pulled(dir.path()), "signed in again: pulled again");
    }
}
