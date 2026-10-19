//! A parameter is a name and its type.
#![allow(unused)]
use ark5::author::{self as ark, Int, Text};

#[ark::helper]
fn f((a, b): (Int, Int)) -> Int {
    a
}

fn main() {}
