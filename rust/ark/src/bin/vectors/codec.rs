//! `codec/` (§1.3) and `order/` (§1.2): values and their canonical bytes,
//! and values and their one total order.

use ark::canon::encode;
use ark::value::{compare_value, hex, Value};

use super::json::{json, obj, quoted};
use super::Out;

/// A value and its bytes per file, and one wrong expectation under
/// `falsify/` that a runner must fail on.
pub fn codec(out: &Out) {
    out.dir("codec/");
    let cases: Vec<(&str, Value)> = vec![
        ("null", Value::Null),
        ("true", Value::Bool(true)),
        ("int-0", Value::Int(0)),
        ("int-24", Value::Int(24)),
        ("int-neg-1", Value::Int(-1)),
        ("int-min", Value::Int(i64::MIN)),
        ("int-max", Value::Int(i64::MAX)),
        ("text-water", Value::text("水")),
        ("text-astral", Value::text("\u{10151}")),
        ("bytes", Value::Bytes(vec![1, 2, 3, 4])),
        ("id-nil", Value::Id([0; 16])),
        ("list", Value::List(vec![Value::Int(1), Value::List(vec![Value::Int(2), Value::Int(3)])])),
        ("struct-key-order", Value::record(vec![("aa", Value::Int(1)), ("b", Value::Int(2))])),
    ];
    for (name, v) in cases {
        out.write(
            &format!("codec/{name}.json"),
            &obj(&[("value", json(&v)), ("bytes", quoted(&hex(&encode(&v))))]),
        );
    }
    out.write(
        "codec/falsify/int-1-wrong-bytes.json",
        &obj(&[("value", json(&Value::Int(1))), ("bytes", quoted("02")), ("expect", quoted("fail"))]),
    );
}

/// Eleven values of every rank, sorted: text by code point, never by
/// UTF-16 unit or by canonical equivalence.
pub fn order(out: &Out) {
    out.dir("order/");
    let input = vec![
        Value::text("e\u{0301}"), // e + combining acute: NOT equal to é
        Value::text("é"),
        Value::text("\u{FF5E}"), // fullwidth tilde: after U+1F3B5 in UTF-16 units, before in code points
        Value::text("\u{1F3B5}"),
        Value::Int(3),
        Value::Null,
        Value::Bool(false),
        Value::List(vec![]),
        Value::List(vec![Value::Int(1)]),
        Value::Bytes(vec![]),
        Value::Struct(Default::default()),
    ];
    let mut sorted = input.clone();
    sorted.sort_by(compare_value);
    // The same values in UTF-16 order, which puts the astral note before
    // the fullwidth tilde.
    let mut utf16 = sorted.clone();
    let at = |v: &str| utf16.iter().position(|x| *x == Value::text(v)).expect("both texts are in the list");
    let (a, b) = (at("\u{FF5E}"), at("\u{1F3B5}"));
    utf16.swap(a, b);
    out.write(
        "order/falsify/utf16.json",
        &obj(&[
            ("input", json(&Value::List(input.clone()))),
            ("sorted", json(&Value::List(utf16))),
            ("expect", quoted("fail")),
        ]),
    );
    out.write(
        "order/mixed.json",
        &obj(&[("input", json(&Value::List(input))), ("sorted", json(&Value::List(sorted)))]),
    );
}
