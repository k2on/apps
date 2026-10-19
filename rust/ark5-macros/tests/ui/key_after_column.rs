//! A table's key is its leading columns.
#![allow(unused)]
use ark5::author::{Int, Row, Text};

#[derive(Row)]
pub struct X {
    pub a: Int,
    #[ark(key)]
    pub b: Text,
}

fn main() {}
