//! How long the demo's reads take, printed rather than asserted: the numbers
//! behind the choices in `harken_domain`'s widest queries. Ignored by
//! default; run with
//! `cargo test -p harken-iced --features demo --release -- --ignored --nocapture bench`.

use std::time::Instant;

use ark_client::{args, Value};

#[test]
#[ignore]
fn bench_queries() {
    let t0 = Instant::now();
    let app = super::demo_app();
    eprintln!("demo seeded and first read in {:.1?}", t0.elapsed());
    let client = &app.peer.client;
    let t = |s: &str| Value::text(s);
    let reads: Vec<(&str, ark_client::Args)> = vec![
        ("library", args([("playlist_id", Value::Id([0; 16]))])),
        ("albums", args([])),
        ("artists", args([])),
        ("composers", args([])),
        ("track_details", args([])),
        ("works", args([("composer", t("Johann Sebastian Bach"))])),
        ("album", args([("playlist_id", Value::Id([0; 16])), ("name", t("Water Music"))])),
        ("playlists", args([])),
    ];
    for (name, a) in &reads {
        let started = Instant::now();
        let v = client.query(name, a).unwrap_or_else(|e| panic!("{name}: {e}"));
        let n = match &v {
            Value::List(xs) => xs.len(),
            _ => 0,
        };
        eprintln!("{name:>14}: {:>10.1?}  ({n} rows)", started.elapsed());
    }
}
