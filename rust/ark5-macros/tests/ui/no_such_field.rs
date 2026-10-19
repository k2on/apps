//! A field is read by name: rustc names a misspelled one.
#![allow(unused)]
use ark5::author::{self as ark, Input, Int};

#[derive(Input)]
pub struct R {
    pub count: Int,
}

#[ark::helper]
fn f(r: R) -> Int {
    r.cuont
}

fn main() {}
