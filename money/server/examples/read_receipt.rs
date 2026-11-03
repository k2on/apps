//! `receipt-ocr PHOTO > boxes.json; cargo run --example read_receipt boxes.json`:
//! what money-server reads of a receipt from what the OCR saw.

fn main() {
    let path = std::env::args().nth(1).expect("usage: read_receipt BOXES.json");
    let doc: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
    let readings: Vec<money_server::paper::Reading> = serde_json::from_value(doc["readings"].clone()).unwrap();
    let p = money_server::paper::read(&readings);
    println!("{} {:?}", p.store, p.date);
    for l in p.items.iter().chain(&p.discounts) {
        println!("  {:<28} {:>8.2}  {}", l.name, l.cents as f64 / 100.0, l.detail);
    }
    println!("sum {:.2} target {:?} {:?} {:?}", p.sum as f64 / 100.0, p.target, p.state, p.changed);
}
