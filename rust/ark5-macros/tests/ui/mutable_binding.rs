//! A binding is never reassigned: `let`, not `let mut`.
#![allow(unused)]
use ark5::author::{self as ark, Int, Text};

#[ark::helper]
fn f(a: Int) -> Int {
    let mut x = a;
    x
}

fn main() {}
