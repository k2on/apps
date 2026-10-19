//! A method is one of the standard library's or a combinator.
#![allow(unused)]
use ark5::author::{self as ark, Int, Text};

#[ark::helper]
fn f(xs: Vec<Int>) -> Vec<Int> {
    xs.push(1)
}

fn main() {}
