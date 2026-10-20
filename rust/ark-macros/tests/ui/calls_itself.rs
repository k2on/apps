//! A function calls another by its hash, which cannot contain itself.
#![allow(unused)]
use ark::author::{Int, Text};

#[ark::helper]
fn f(a: Int) -> Int {
    f(a)
}

fn main() {}
