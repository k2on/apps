//! A body is its value: no `return`.
#![allow(unused)]
use ark::author::{Int, Text};

#[ark::helper]
fn f(a: Int) -> Int {
    return a;
}

fn main() {}
