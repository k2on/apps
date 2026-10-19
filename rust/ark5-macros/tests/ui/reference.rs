//! Values, never references.
#![allow(unused)]
use ark5::author::{self as ark, Int, Text};

#[ark::helper]
fn f(a: Int) -> Int {
    let b = &a;
    a
}

fn main() {}
