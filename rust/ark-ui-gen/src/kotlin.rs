//! Compose, as the Android app's hand-written screens were.

use std::fmt::Debug;

use crate::*;

/// The Kotlin file for `view`.
pub fn emit<M: Debug>(_view: &View<M>, s: &Screen) -> String {
    header("//", s)
}
