//! No `?`: `refuse!` is how a function refuses.
#![allow(unused)]
use ark5::author::{self as ark, Int, Text};

#[ark::helper]
fn f(o: Option<Int>) -> Int {
    o?
}

fn main() {}
