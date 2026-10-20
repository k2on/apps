//! A parameter is a name and its type.
#![allow(unused)]
use ark::author::{Int, Text};

#[ark::helper]
fn f((a, b): (Int, Int)) -> Int {
    a
}

fn main() {}
