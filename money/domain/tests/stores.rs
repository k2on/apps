//! The stores' adapters, on exports shaped as each store's are. Amazon's is
//! what `amazon-orders` (5.x) prints, made by its own serializer; Costco's
//! and Target's are their pages' JSON as community scripts have recorded
//! it, and as near as is known — the adapters read the spellings seen.

use money_domain::stores::{amazon, costco, read, target};

fn items(r: &money_domain::StoreReceipt) -> Vec<(&str, &str, i64, i64)> {
    r.items.iter().map(|i| (i.name.as_str(), i.sku.as_str(), i.quantity, i.amount)).collect()
}

/// An order a receipt, cancelled ones left out, a shipment's items when
/// the order lists none, and the program's last line not JSON.
#[test]
fn amazon_orders_prints_orders() {
    let got = amazon(include_str!("stores/amazon-orders.json")).unwrap();
    assert_eq!(got.store, "amazon");
    let ids: Vec<&str> = got.receipts.iter().map(|r| r.order_id.as_str()).collect();
    assert_eq!(ids, ["112-1234567-1234567", "112-7654321-7654321"]);
    let r = &got.receipts[0];
    assert_eq!((r.date.as_str(), r.total), ("2026-10-02", -1_653));
    assert_eq!(
        items(r),
        [
            ("Vital Farms Pasture-Raised Eggs, 12 Count", "B07XYZ", 1000, -649),
            ("Scotch-Brite Sponges, 9 pack", "B000PCWKV2", 2000, -900)
        ]
    );
    assert_eq!(items(&got.receipts[1]), [("USB-C cable", "B0C", 1000, -899)]);
    assert!(amazon("Order ID,Order Date\n1,2026-10-01").is_err());
}

/// A warehouse receipt's items with its coupon, and a gas station's fuel in
/// gallons, named so a category "Gas" finds it.
#[test]
fn costco_warehouse_and_gas() {
    let got = costco(include_str!("stores/costco.json")).unwrap();
    let (shop, gas) = (&got.receipts[0], &got.receipts[1]);
    assert_eq!(
        (shop.date.as_str(), shop.total, shop.place.as_str()),
        ("2026-10-12", -4_720, "Costco MAPLEWOOD")
    );
    assert_eq!(
        items(shop),
        [
            ("KS ORGANIC EGGS 24CT", "1234", 2000, -1_398),
            ("KS PAPER TOWEL", "5678", 1000, -2_299),
            ("/5678", "345", -1000, 400),
            ("BANANAS", "9012", 1000, -1_200),
        ]
    );
    assert_eq!((gas.total, gas.place.as_str()), (-4_127, "Costco Gas MAPLEWOOD"));
    assert_eq!(items(gas), [("Gas REGULAR", "800", 11_482, -4_127)]);
    assert_eq!(gas.items[0].unit, "gal");
}

/// What `download_costco_receipts.js` (ankurdave/beancount_import_sources)
/// saves: the `receipts` query's answer, a bare array, its fields as that
/// script asks for them.
#[test]
fn costco_as_the_receipts_script_saves_them() {
    let got = costco(include_str!("stores/costco-receipts-script.json")).unwrap();
    let r = &got.receipts[0];
    assert_eq!(
        (r.order_id.as_str(), r.date.as_str(), r.total),
        ("21134300501232510121504", "2026-10-12", -4_720)
    );
    assert_eq!(
        items(r),
        [
            ("KS ORGANIC EGGS 24CT", "1234", 2000, -1_398),
            ("KS PAPER TOWEL", "5678", 1000, -2_299),
            ("/5678", "345678", -1000, 400),
            ("BANANAS", "9012", 1000, -1_200),
        ]
    );
}

/// A line's price as a total, or as one's times how many; text or number.
#[test]
fn target_orders() {
    let got = read("target", include_str!("stores/target.json")).unwrap();
    let r = &got.receipts[0];
    assert_eq!(
        (r.order_id.as_str(), r.date.as_str(), r.total, r.place.as_str()),
        ("9120004511", "2026-10-25", -8_740, "Target Maplewood")
    );
    assert_eq!(
        items(r),
        [
            ("Good & Gather Large Eggs, 12ct", "14713534", 1000, -429),
            ("Dutch Boy Interior Paint", "53212", 1000, -3_599),
            ("Snacks", "", 3000, -4_182),
        ]
    );
    assert!(target("not json").unwrap_err().contains("not JSON"));
    assert!(read("walmart", "").is_err());
}
