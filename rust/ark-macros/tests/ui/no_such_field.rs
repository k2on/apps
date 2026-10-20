//! A field is read by name: rustc names a misspelled one.
#![allow(unused)]
use ark::author::{Input, Int};

#[derive(Input)]
pub struct R {
    pub count: Int,
}

#[ark::helper]
fn f(r: R) -> Int {
    r.cuont
}

fn main() {}
