//! A `let` binds one name.
#![allow(unused)]
use ark::author::{Int, Text};

#[ark::helper]
fn f(a: Int) -> Int {
    let (x, y) = (a, a);
    x
}

fn main() {}
