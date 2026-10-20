//! Nothing is assigned: a value is a new one.
#![allow(unused)]
use ark::author::{Int, Text};

#[ark::helper]
fn f(a: Int) -> Int {
    a += 1;
    a
}

fn main() {}
