//! A body is statements and expressions; a function is an item of its own.
#![allow(unused)]
use ark::author::{Int, Text};

#[ark::helper]
fn f(a: Int) -> Int {
    fn g() {}
    a
}

fn main() {}
