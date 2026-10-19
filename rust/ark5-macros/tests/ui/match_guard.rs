//! An arm has no guard: an `if` in it is.
#![allow(unused)]
use ark5::author::{self as ark, Int, Text};

#[ark::helper]
fn f(o: Option<Int>) -> Int {
    match o {
        Some(x) if x > 0 => x,
        _ => 0,
    }
}

fn main() {}
