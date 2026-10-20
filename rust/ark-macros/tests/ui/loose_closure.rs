//! A closure is a combinator's argument, and nothing else.
#![allow(unused)]
use ark::author::{Int, Text};

#[ark::helper]
fn f(a: Int) -> Int {
    let g = |x: Int| x;
    a
}

fn main() {}
