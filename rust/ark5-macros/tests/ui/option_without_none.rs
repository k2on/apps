//! A match on an option has both arms, or `_`.
#![allow(unused)]
use ark5::author::{self as ark, Int, Text};

#[ark::helper]
fn f(o: Option<Int>) -> Int {
    match o {
        Some(x) => x,
    }
}

fn main() {}
