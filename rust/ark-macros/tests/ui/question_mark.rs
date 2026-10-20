//! No `?`: `refuse!` is how a function refuses.
#![allow(unused)]
use ark::author::{Int, Text};

#[ark::helper]
fn f(o: Option<Int>) -> Int {
    o?
}

fn main() {}
