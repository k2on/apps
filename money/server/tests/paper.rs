//! Photos of paper receipts, read: the OCR's boxes of four real receipts
//! (tests/paper/, each at three detector sizes; what named a person or a
//! card taken out) against what each says, item by item.

use money_server::paper::{self, State};
use serde_json::Value;

fn readings(name: &str) -> Vec<paper::Reading> {
    [736, 1280, 1920]
        .iter()
        .map(|side| {
            let path = format!("{}/tests/paper/{name}.{side}.json", env!("CARGO_MANIFEST_DIR"));
            serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap()
        })
        .collect()
}

fn truth(name: &str) -> Value {
    let path = format!("{}/tests/paper/{name}.truth.json", env!("CARGO_MANIFEST_DIR"));
    serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap()
}

fn cents(v: &Value) -> i64 {
    (v.as_f64().unwrap() * 100.0).round() as i64
}

/// Every item at its price, in order, every discount, and sure of it.
fn reads_whole(name: &str) {
    let p = paper::read(&readings(name));
    let t = truth(name);
    let want: Vec<i64> = t["items"].as_array().unwrap().iter().map(|i| cents(&i[2])).collect();
    let got: Vec<i64> = p.items.iter().map(|i| i.cents).collect();
    assert_eq!(got, want, "{name}: {:#?}", p.items.iter().map(|i| (&i.name, i.cents)).collect::<Vec<_>>());
    let want: Vec<i64> = t["discounts"]
        .as_array()
        .map(|d| d.iter().map(|d| cents(&d[1])).collect())
        .unwrap_or_default();
    let got: Vec<i64> = p.discounts.iter().map(|d| d.cents).collect();
    assert_eq!(got, want, "{name}: discounts");
    assert_eq!(p.state, State::Sure, "{name}: {:?} {:?}", p.state, p.changed);
    assert_eq!(p.sum, p.target.unwrap());
}

#[test]
fn aldi_twenty_eight_lines_and_a_tax_code_on_each() {
    reads_whole("aldi-2026-11-02");
    let p = paper::read(&readings("aldi-2026-11-02"));
    assert_eq!((p.subtotal, p.tax), (Some(9880), 25));
    assert!(
        p.items[27].detail.contains("2.09"),
        "the broccoli's weight under it: {}",
        p.items[27].detail
    );
}

#[test]
fn food_lion_departments_quantities_and_what_was_saved() {
    reads_whole("foodlion-2025-05-27");
    let p = paper::read(&readings("foodlion-2025-05-27"));
    assert_eq!(p.due, Some(3736), "no subtotal: the balance due, less the tax");
    assert!(p.items.iter().any(|i| i.name.contains("SEASONING") && i.detail.contains("1.19")));
}

#[test]
fn food_lion_with_tax() {
    reads_whole("foodlion-2025-06-15");
    let p = paper::read(&readings("foodlion-2025-06-15"));
    assert_eq!((p.tax, p.due, p.target), (23, Some(3240), Some(3217)));
}

#[test]
fn a_blurred_restaurant_bill_with_counts() {
    reads_whole("kips-2026-06-05");
    let p = paper::read(&readings("kips-2026-06-05"));
    let pie = p.items.iter().find(|i| i.cents == 3400).unwrap();
    assert_eq!(pie.qty, Some(2), "{pie:?}");
}

/// A reading a line short does not add up, and is checked, not guessed.
#[test]
fn a_line_missing_is_checked() {
    let mut r = readings("foodlion-2025-06-15");
    for reading in &mut r {
        reading.retain(|w| !w.text.contains("15.11"));
    }
    let p = paper::read(&r);
    assert_eq!(p.state, State::Check);
    assert_ne!(p.sum, p.target.unwrap());
}
