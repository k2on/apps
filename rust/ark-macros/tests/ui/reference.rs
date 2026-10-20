//! Values, never references.
#![allow(unused)]
use ark::author::{Int, Text};

#[ark::helper]
fn f(a: Int) -> Int {
    let b = &a;
    a
}

fn main() {}
