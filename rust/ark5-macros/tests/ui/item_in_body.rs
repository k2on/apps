//! A body is statements and expressions; a function is an item of its own.
#![allow(unused)]
use ark5::author::{self as ark, Int, Text};

#[ark::helper]
fn f(a: Int) -> Int {
    fn g() {}
    a
}

fn main() {}
