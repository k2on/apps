//! Nothing is assigned: a value is a new one.
#![allow(unused)]
use ark5::author::{self as ark, Int, Text};

#[ark::helper]
fn f(a: Int) -> Int {
    a += 1;
    a
}

fn main() {}
