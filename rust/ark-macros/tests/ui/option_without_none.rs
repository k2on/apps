//! A match on an option has both arms, or `_`.
#![allow(unused)]
use ark::author::{Int, Text};

#[ark::helper]
fn f(o: Option<Int>) -> Int {
    match o {
        Some(x) => x,
    }
}

fn main() {}
