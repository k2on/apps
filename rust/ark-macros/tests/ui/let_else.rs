//! `let … else`: a `match` is.
#![allow(unused)]
use ark::author::{Int, Text};

#[ark::helper]
fn f(a: Int) -> Int {
    let x = a else { return 0 };
    x
}

fn main() {}
