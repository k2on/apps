//! `#[ark(..)]` says a refinement, a key, an index, a reference.
#![allow(unused)]
use ark::author::{Input, Text};

#[derive(Input)]
pub struct X {
    #[ark(trimmed)]
    pub a: Text,
}

fn main() {}
