//! The operators are checked arithmetic, comparison and the booleans.
#![allow(unused)]
use ark5::author::{self as ark, Int, Text};

#[ark::helper]
fn f(a: Int) -> Int {
    a & 1
}

fn main() {}
