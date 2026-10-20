//! A tuple is of two to four.
#![allow(unused)]
use ark::author::{Int, Text};

#[ark::helper]
fn f(a: Int) -> (Int, Int, Int, Int, Int) {
    (a, a, a, a, a)
}

fn main() {}
