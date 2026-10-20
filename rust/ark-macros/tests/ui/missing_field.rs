//! A struct literal names every field, or `..base`: rustc names the one
//! missing.
#![allow(unused)]
use ark::author::{Input, Int};

#[derive(Input)]
pub struct R {
    pub a: Int,
    pub b: Int,
}

#[ark::helper]
fn f(x: Int) -> R {
    R { a: x }
}

fn main() {}
