//! A pattern names a variant and its fields; a match in the arm is the rest.
#![allow(unused)]
use ark5::author::{self as ark, Int, Text};

#[ark::helper]
fn f(o: Option<Option<Int>>) -> Int {
    match o {
        Some(Some(x)) => x,
        _ => 0,
    }
}

fn main() {}
