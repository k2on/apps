//! There is one integer.
#![allow(unused)]
use ark::author::{Int, Text};

#[ark::helper]
fn f(a: Int) -> Int {
    a as i64
}

fn main() {}
