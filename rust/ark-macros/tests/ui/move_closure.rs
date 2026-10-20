//! A closure captures nothing; it binds its parameters.
#![allow(unused)]
use ark::author::{Int, Text};

#[ark::helper]
fn f(xs: Vec<Int>) -> Vec<Int> {
    xs.map(move |x| x)
}

fn main() {}
