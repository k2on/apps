//! A combinator's types are its arguments'.
#![allow(unused)]
use ark5::author::{self as ark, Int, Text};

#[ark::helper]
fn f(xs: Vec<Int>) -> Vec<Int> {
    xs.map::<Int>(|x| x)
}

fn main() {}
