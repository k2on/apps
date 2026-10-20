//! A binding is never reassigned: `let`, not `let mut`.
#![allow(unused)]
use ark::author::{Int, Text};

#[ark::helper]
fn f(a: Int) -> Int {
    let mut x = a;
    x
}

fn main() {}
