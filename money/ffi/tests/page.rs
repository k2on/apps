#![cfg(feature = "page")]
//! Where a receipt is in a camera frame (src/page.rs): pages drawn on a
//! table, a counter and clutter (as the scanner's bench drew them: a
//! printed receipt warped into the scene), each found at the corners it was
//! drawn at.

use money_ffi::money_page_corners;

/// Each scene, and the corners its receipt was drawn at, x then y from 0
/// to 1, clockwise from the top left.
const PAGES: [(&str, [(f32, f32); 4]); 3] = [
    ("dark wood", [(0.373, 0.206), (0.633, 0.184), (0.669, 0.792), (0.412, 0.820)]),
    ("white counter", [(0.185, 0.250), (0.455, 0.184), (0.592, 0.748), (0.306, 0.804)]),
    ("clutter", [(0.260, 0.168), (0.599, 0.173), (0.556, 0.827), (0.180, 0.752)]),
];
const FILES: [&str; 3] = ["page-0.png", "page-1.png", "page-3.png"];

fn rgb(file: &str) -> Vec<u8> {
    let path = format!("{}/tests/pages/{file}", env!("CARGO_MANIFEST_DIR"));
    image::open(&path).unwrap().to_rgb8().into_raw()
}

/// Falsified by reading a heatmap's rows as its columns: the corners land
/// mirrored across the diagonal.
#[test]
fn a_receipt_is_found_at_its_corners_on_a_table_a_counter_and_clutter() {
    for ((kind, drawn), file) in PAGES.iter().zip(FILES) {
        let got = money_page_corners(rgb(file)).unwrap_or_else(|| panic!("{kind}: no page found"));
        for (i, (x, y)) in drawn.iter().enumerate() {
            let (gx, gy) = (got[2 * i], got[2 * i + 1]);
            assert!(
                (gx - x).abs() < 0.04 && (gy - y).abs() < 0.04,
                "{kind}, corner {i}: drawn {x},{y}, found {gx},{gy}"
            );
        }
    }
}

#[test]
fn a_picture_of_another_size_is_no_page() {
    assert_eq!(money_page_corners(vec![0; 10]), None);
}
