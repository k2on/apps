//! `#[ark(..)]` says a refinement, a key, an index, a reference.
#![allow(unused)]
use ark5::author::{Input, Text};

#[derive(Input)]
pub struct X {
    #[ark(trimmed)]
    pub a: Text,
}

fn main() {}
