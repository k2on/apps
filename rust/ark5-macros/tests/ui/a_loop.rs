//! There is no loop: a list's `map` is.
#![allow(unused)]
use ark5::author::{self as ark, Int, Text};

#[ark::helper]
fn f(a: Int) -> Int {
    while a > 0 {}
    a
}

fn main() {}
