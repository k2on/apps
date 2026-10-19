//! A body is its value: no `return`.
#![allow(unused)]
use ark5::author::{self as ark, Int, Text};

#[ark::helper]
fn f(a: Int) -> Int {
    return a;
}

fn main() {}
