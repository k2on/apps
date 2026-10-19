//! A `let` binds one name.
#![allow(unused)]
use ark5::author::{self as ark, Int, Text};

#[ark::helper]
fn f(a: Int) -> Int {
    let (x, y) = (a, a);
    x
}

fn main() {}
