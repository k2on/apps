//! The stores' adapters: each reads what a store gives a customer of their
//! own purchases and makes it one [`ReceiptImport`] in cents and our sign
//! (spent is negative), which [`crate::receipts::add_receipts`] applies.
//!
//! None of the stores has an API for its customers; these read the exports
//! a customer can take themselves:
//!
//! - **Amazon** ([`amazon`]): what the `amazon-orders` program prints
//!   (`amazon-orders orders --full-details -o json`), which the server's
//!   importer runs. An order is a receipt; cancelled ones are left out.
//! - **Target** ([`target`]): the JSON target.com's order history loads —
//!   online orders and, with Target Circle, in-store receipts — as the
//!   browser's developer tools save it.
//! - **Costco** ([`costco`]): the JSON costco.com's Orders & Purchases page
//!   loads for warehouse and gas station receipts (its `receiptsWithCounts`
//!   answer), saved the same way.
//!
//! The stores' JSON is their own and unpublished: each adapter reads the
//! names it has been seen with and the few spellings like them, and says
//! which receipt it could not read rather than guessing. Every amount may
//! be a number of dollars or text (`$4.29`, `1,204.10`).

use serde_json::Value as Json;

use crate::types::{ChargeImport, ReceiptImport, StoreCharge, StoreItem, StoreReceipt};

/// The stores there are adapters for.
pub const STORES: [&str; 3] = ["amazon", "target", "costco"];

/// One export read by its store's adapter.
pub fn read(store: &str, text: &str) -> Result<ReceiptImport, String> {
    match store {
        "amazon" => amazon(text),
        "target" => target(text),
        "costco" => costco(text),
        _ => Err(format!("no adapter for {store}: amazon, target or costco")),
    }
}

// * Reading

/// Dollars, as a number or as text, in cents; none for what is not one
/// (`Not Available`, blank).
pub fn cents_of(v: &Json) -> Option<i64> {
    match v {
        Json::Number(n) => n.as_f64().map(dollars),
        Json::String(s) => cents_in(s),
        _ => None,
    }
}

/// Dollars written as text, in cents: `$1,204.10`, `-3.00`, `(3.00)`.
pub fn cents_in(s: &str) -> Option<i64> {
    let t = s.trim();
    let (neg, t) = match t.strip_prefix('(').and_then(|t| t.strip_suffix(')')) {
        Some(inner) => (true, inner),
        None => (false, t),
    };
    let clean: String = t.chars().filter(|c| !matches!(c, '$' | ',' | ' ')).collect();
    let n: f64 = clean.parse().ok()?;
    Some(dollars(if neg { -n } else { n }))
}

fn dollars(n: f64) -> i64 {
    (n * 100.0).round() as i64
}

/// A count, as a number or text, in thousandths; one when none.
fn thousandths(v: Option<&Json>) -> i64 {
    let n = match v {
        Some(Json::Number(n)) => n.as_f64(),
        Some(Json::String(s)) => s.trim().parse().ok(),
        _ => None,
    };
    n.filter(|n| n.is_finite() && *n != 0.0).map_or(1000, |n| (n * 1000.0).round() as i64)
}

/// The day a store's date or time is: `2026-10-25T14:33:00Z`, `2026-10-25`,
/// `10/25/2026`, `10/25/26`.
pub fn day_of(s: &str) -> Option<String> {
    let s = s.trim();
    if s.len() >= 10 && crate::dates::is_day(&s[..10]) {
        return Some(s[..10].to_string());
    }
    let date = s.split([' ', 'T']).next()?;
    let parts: Vec<&str> = date.split('/').collect();
    if let [m, d, y] = parts[..] {
        let y: i64 = y.parse().ok()?;
        let y = if y < 100 { 2000 + y } else { y };
        let day = format!("{y:04}-{:02}-{:02}", m.parse::<u32>().ok()?, d.parse::<u32>().ok()?);
        return crate::dates::is_day(&day).then_some(day);
    }
    None
}

/// The first of `paths` (dotted: `summary.grand_total`) that `v` has, and
/// is not null.
fn dig<'a>(v: &'a Json, paths: &[&str]) -> Option<&'a Json> {
    paths.iter().find_map(|p| {
        let mut at = v;
        for k in p.split('.') {
            at = at.get(k)?;
        }
        (!at.is_null()).then_some(at)
    })
}

fn field(v: &Json, paths: &[&str]) -> String {
    match dig(v, paths) {
        Some(Json::String(s)) => s.trim().to_string(),
        Some(Json::Number(n)) => n.to_string(),
        _ => String::new(),
    }
}

/// The first array of `paths` that `v` has; or `v`, when it is one.
fn list<'a>(v: &'a Json, paths: &[&str]) -> &'a [Json] {
    if let Json::Array(a) = v {
        return a;
    }
    match dig(v, paths) {
        Some(Json::Array(a)) => a,
        _ => &[],
    }
}

// * Amazon

/// The first JSON value of `text`, and whatever follows it left alone:
/// `amazon-orders` says how many orders it read after the list.
fn first_json(text: &str) -> Result<Json, String> {
    let text = text.trim_start().trim_start_matches('\u{feff}');
    serde_json::Deserializer::from_str(text)
        .into_iter::<Json>()
        .next()
        .ok_or_else(|| "nothing".to_string())?
        .map_err(|e| e.to_string())
}

/// What `amazon-orders orders --full-details -o json` prints: its `Order`s,
/// each `order_number`, `order_placed_date`, `grand_total` (dollars, tax
/// in), `cancelled`, and `items` (else its `shipments`' items), each
/// `title`, `asin`, `price` (one's, before tax) and `quantity`.
pub fn amazon(text: &str) -> Result<ReceiptImport, String> {
    let v = first_json(text).map_err(|e| format!("amazon: not amazon-orders' JSON: {e}"))?;
    let mut receipts = vec![];
    for (n, o) in list(&v, &["orders", "items"]).iter().enumerate() {
        if dig(o, &["cancelled"]).and_then(Json::as_bool) == Some(true) {
            continue;
        }
        let id = field(o, &["order_number", "order_id"]);
        if id.is_empty() {
            return Err(format!("amazon: order {}: no order number", n + 1));
        }
        let raw = field(o, &["order_placed_date", "order_date"]);
        let date = day_of(&raw).ok_or_else(|| format!("amazon: order {id}: `{raw}` is not a date"))?;
        let mut lines: Vec<&Json> = list(o, &["items"]).iter().collect();
        if lines.is_empty() {
            lines = list(o, &["shipments"]).iter().flat_map(|s| list(s, &["items"])).collect();
        }
        let items: Vec<StoreItem> = lines
            .into_iter()
            .map(|i| {
                let quantity = thousandths(dig(i, &["quantity"]));
                let each = dig(i, &["price"]).and_then(cents_of).unwrap_or(0);
                StoreItem {
                    name: field(i, &["title"]),
                    sku: field(i, &["asin"]),
                    quantity,
                    unit: "ea".into(),
                    amount: -(each * quantity / 1000),
                }
            })
            .collect();
        let total = dig(o, &["grand_total"])
            .and_then(cents_of)
            .map(|t| -t)
            .unwrap_or_else(|| items.iter().map(|i| i.amount).sum());
        receipts.push(StoreReceipt {
            order_id: id,
            date,
            place: match dig(o, &["is_whole_foods"]).and_then(Json::as_bool) {
                Some(true) => "Whole Foods".into(),
                _ => "Amazon.com".into(),
            },
            total,
            items,
        });
    }
    Ok(ReceiptImport {
        store: "amazon".into(),
        receipts,
    })
}

/// What `amazon-orders transactions --days N -o json` prints: every
/// charge and refund, each `completed_date`, `grand_total` (dollars,
/// negative for a charge, as our sign), `order_number` and
/// `payment_method`. One with no order (a gift card's reload) is left out.
pub fn amazon_charges(text: &str) -> Result<ChargeImport, String> {
    let v = first_json(text).map_err(|e| format!("amazon: not amazon-orders' JSON: {e}"))?;
    let mut charges = vec![];
    for (n, t) in list(&v, &["transactions", "items"]).iter().enumerate() {
        let order_id = field(t, &["order_number"]);
        if order_id.is_empty() {
            continue;
        }
        let raw = field(t, &["completed_date", "date"]);
        let date = day_of(&raw).ok_or_else(|| format!("amazon: transaction {}: `{raw}` is not a date", n + 1))?;
        let amount = dig(t, &["grand_total"])
            .and_then(cents_of)
            .ok_or_else(|| format!("amazon: transaction {} of order {order_id}: no amount", n + 1))?;
        charges.push(StoreCharge {
            order_id,
            date,
            amount,
            card: field(t, &["payment_method"]),
        });
    }
    Ok(ChargeImport {
        store: "amazon".into(),
        charges,
    })
}

// * Target

/// Target's order history, from the JSON its pages load: an array of
/// orders, or an object holding one (`orders`, `order_history`, `data`).
pub fn target(text: &str) -> Result<ReceiptImport, String> {
    let v: Json = serde_json::from_str(text).map_err(|e| format!("target: not JSON: {e}"))?;
    let orders = list(&v, &["orders", "order_history", "data.orders", "data"]);
    let mut receipts = vec![];
    for (n, o) in orders.iter().enumerate() {
        let id = field(o, &["order_number", "orderNumber", "store_receipt_id", "receipt_id", "id"]);
        if id.is_empty() {
            return Err(format!("target: order {}: no order number", n + 1));
        }
        let raw = field(
            o,
            &["placed_date", "order_date", "orderDate", "purchase_date", "transaction_date", "date"],
        );
        let date = day_of(&raw).ok_or_else(|| format!("target: order {id}: `{raw}` is not a date"))?;
        let mut items = vec![];
        for l in list(o, &["order_lines", "orderLines", "lines", "items"]) {
            let quantity = thousandths(dig(l, &["quantity", "ordered_quantity", "original_quantity", "qty"]));
            let amount = dig(l, &["total_price", "line_total", "item_total", "price_total", "summary.total"])
                .and_then(cents_of)
                .or_else(|| {
                    let unit = dig(l, &["unit_price", "item.unit_price", "price", "list_price"]).and_then(cents_of)?;
                    Some(unit * quantity / 1000)
                })
                .ok_or_else(|| format!("target: order {id}: a line with no price"))?;
            items.push(StoreItem {
                name: field(l, &["item.description", "item.product_description.title", "description", "name", "title"]),
                sku: field(l, &["item.tcin", "tcin", "item.dpci", "dpci"]),
                quantity,
                unit: "ea".into(),
                amount: -amount,
            });
        }
        let total = dig(o, &["summary.grand_total", "grand_total", "summary.total", "total", "order_total"])
            .and_then(cents_of)
            .map(|t| -t)
            .unwrap_or_else(|| items.iter().map(|i| i.amount).sum());
        receipts.push(StoreReceipt {
            order_id: id,
            date,
            place: match field(o, &["store.location_name", "store_name", "location_name", "store.name"]) {
                s if s.is_empty() => "Target".into(),
                s => format!("Target {s}"),
            },
            total,
            items,
        });
    }
    Ok(ReceiptImport {
        store: "target".into(),
        receipts,
    })
}

// * Costco

/// Costco's warehouse and gas receipts, from the JSON its Orders &
/// Purchases page loads (`data.receiptsWithCounts.receipts`), or an array
/// of receipts. A gas station's lines are its fuel, in gallons, named so
/// that a category called "Gas" finds them.
pub fn costco(text: &str) -> Result<ReceiptImport, String> {
    let v: Json = serde_json::from_str(text).map_err(|e| format!("costco: not JSON: {e}"))?;
    let all = list(
        &v,
        &[
            "data.receiptsWithCounts.receipts",
            "receiptsWithCounts.receipts",
            "data.receipts",
            "receipts",
        ],
    );
    let mut receipts = vec![];
    for (n, r) in all.iter().enumerate() {
        let id = field(r, &["transactionBarcode", "barcode", "receiptId"]);
        if id.is_empty() {
            return Err(format!("costco: receipt {}: no barcode", n + 1));
        }
        let raw = field(r, &["transactionDateTime", "transactionDate", "date"]);
        let date = day_of(&raw).ok_or_else(|| format!("costco: receipt {id}: `{raw}` is not a date"))?;
        let kind = format!("{} {}", field(r, &["receiptType"]), field(r, &["documentType"])).to_lowercase();
        let gas = kind.contains("gas") || kind.contains("fuel");
        let mut items = vec![];
        for i in list(r, &["itemArray", "items"]) {
            let name = format!("{} {}", field(i, &["itemDescription01", "description"]), field(i, &["itemDescription02"]))
                .trim()
                .to_string();
            let amount = dig(i, &["amount", "price"])
                .and_then(cents_of)
                .ok_or_else(|| format!("costco: receipt {id}: `{name}` has no amount"))?;
            let gallons = dig(i, &["fuelUnitQuantity"]).filter(|_| gas);
            let name = match gas && !crate::receipts::words(&name).iter().any(|w| w == "ga" || w == "gas") {
                true => format!("Gas {name}").trim().to_string(),
                false => name,
            };
            items.push(StoreItem {
                name,
                sku: field(i, &["itemNumber", "itemIdentifier"]),
                quantity: thousandths(gallons.or_else(|| dig(i, &["unit", "quantity"]))),
                unit: if gallons.is_some() { "gal" } else { "ea" }.into(),
                amount: -amount,
            });
        }
        let total = dig(r, &["total", "totalAmount"])
            .and_then(cents_of)
            .map(|t| -t)
            .ok_or_else(|| format!("costco: receipt {id}: no total"))?;
        receipts.push(StoreReceipt {
            order_id: id,
            date,
            place: match field(r, &["warehouseName", "warehouseShortName"]) {
                s if s.is_empty() => "Costco".into(),
                s if gas => format!("Costco Gas {s}"),
                s => format!("Costco {s}"),
            },
            total,
            items,
        });
    }
    Ok(ReceiptImport {
        store: "costco".into(),
        receipts,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn money_and_days_as_stores_write_them() {
        assert_eq!(cents_in("$1,204.10"), Some(120410));
        assert_eq!(cents_in("(3.00)"), Some(-300));
        assert_eq!(cents_in("Not Available"), None);
        assert_eq!(cents_of(&serde_json::json!(4.29)), Some(429));
        assert_eq!(day_of("2026-10-25T14:33:00.000Z").as_deref(), Some("2026-10-25"));
        assert_eq!(day_of("10/5/26").as_deref(), Some("2026-10-05"));
        assert_eq!(day_of("soon"), None);
    }
}
