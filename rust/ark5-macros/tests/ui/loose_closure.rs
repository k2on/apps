//! A closure is a combinator's argument, and nothing else.
#![allow(unused)]
use ark5::author::{self as ark, Int, Text};

#[ark::helper]
fn f(a: Int) -> Int {
    let g = |x: Int| x;
    a
}

fn main() {}
