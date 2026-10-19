//! The macros are `vec!` and `refuse!`.
#![allow(unused)]
use ark5::author::{self as ark, Int, Text};

#[ark::helper]
fn f() -> Text {
    format!("x")
}

fn main() {}
