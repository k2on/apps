//! There is no loop: a list's `map` is.
#![allow(unused)]
use ark::author::{Int, Text};

#[ark::helper]
fn f(a: Int) -> Int {
    while a > 0 {}
    a
}

fn main() {}
