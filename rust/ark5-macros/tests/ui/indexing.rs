//! A list is not indexed: `slice` and `first` are.
#![allow(unused)]
use ark5::author::{self as ark, Int, Text};

#[ark::helper]
fn f(xs: Vec<Int>) -> Int {
    xs[0]
}

fn main() {}
