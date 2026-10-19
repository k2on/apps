//! A struct literal names every field, or `..base`: rustc names the one
//! missing.
#![allow(unused)]
use ark5::author::{self as ark, Input, Int};

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
