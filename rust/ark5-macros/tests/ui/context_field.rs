//! The context is user, session, roles and now (no device yet).
#![allow(unused)]
use ark5::author::{self as ark, Int, Text};

#[ark::helper]
fn f(ctx: Ctx) -> Text {
    ctx.device
}

fn main() {}
