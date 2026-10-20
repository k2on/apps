//! A combinator's types are its arguments'.
#![allow(unused)]
use ark::author::{Int, Text};

#[ark::helper]
fn f(xs: Vec<Int>) -> Vec<Int> {
    xs.map::<Int>(|x| x)
}

fn main() {}
