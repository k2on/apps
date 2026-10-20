//! The context is user, session, roles and now (no device yet).
#![allow(unused)]
use ark::author::{Int, Text};

#[ark::helper]
fn f(ctx: Ctx) -> Text {
    ctx.device
}

fn main() {}
