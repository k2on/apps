//! A tuple is of two to four.
#![allow(unused)]
use ark5::author::{self as ark, Int, Text};

#[ark::helper]
fn f(a: Int) -> (Int, Int, Int, Int, Int) {
    (a, a, a, a, a)
}

fn main() {}
