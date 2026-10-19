//! A refinement holds where bytes enter, at parse: nothing inside makes one.
#![allow(unused)]
use ark5::author::{self as ark, Input, Text};

#[derive(Input)]
#[ark(trim, non_empty)]
pub struct Name(Text);

#[ark::helper]
fn f(t: Text) -> Name {
    Name(t)
}

fn main() {}
