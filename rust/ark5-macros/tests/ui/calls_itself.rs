//! A function calls another by its hash, which cannot contain itself.
#![allow(unused)]
use ark5::author::{self as ark, Int, Text};

#[ark::helper]
fn f(a: Int) -> Int {
    f(a)
}

fn main() {}
