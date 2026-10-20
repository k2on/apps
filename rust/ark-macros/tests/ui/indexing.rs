//! A list is not indexed: `slice` and `first` are.
#![allow(unused)]
use ark::author::{Int, Text};

#[ark::helper]
fn f(xs: Vec<Int>) -> Int {
    xs[0]
}

fn main() {}
