//! `let … else`: a `match` is.
#![allow(unused)]
use ark5::author::{self as ark, Int, Text};

#[ark::helper]
fn f(a: Int) -> Int {
    let x = a else { return 0 };
    x
}

fn main() {}
