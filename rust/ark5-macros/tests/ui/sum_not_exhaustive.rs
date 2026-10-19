//! A match on a declared sum has an arm per variant, or `_`: rustc says
//! which is missing.
#![allow(unused)]
use ark5::author::{self as ark, Int, Sum};

#[derive(Sum)]
pub enum E {
    A,
    B { x: Int },
}

#[ark::helper]
fn f(e: E) -> Int {
    match e {
        E::A => 1,
    }
}

fn main() {}
