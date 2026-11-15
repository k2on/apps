//! The notices laid out, once.

use ark_app::shell::*;
use ark_app::view;
use ark_app::view::*;

use super::{NoticesMsg, NoticesPage};

/// An alert while there is one; nothing otherwise.
#[view]
pub fn notices(p: &NoticesPage) -> View<NoticesMsg> {
    if let Some(a) = p.alert {
        alert("Devices", a.words, NoticesMsg::Dismiss)
    }
}
