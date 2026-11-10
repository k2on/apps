//! The page an app is while it signs in: what `ark_auth::onboarding`'s
//! machine says short of the app itself — the app's name, what it waits
//! on ("Waiting for the sign-in…", "Getting your budget from …"), why the
//! last try did not go, and what it offers (Sign In). Every app's page is
//! this one, as the machine is every app's.

use leptos::html;
use leptos::prelude::*;

use crate::parts::push_button;

/// What the sign-in page says and offers.
pub struct Welcome<M> {
    /// The app's name, as a person reads it: `Money`.
    pub title: String,
    /// What it is waiting on, while something is under way.
    pub status: Option<String>,
    /// Why the last try did not go.
    pub error: Option<String>,
    /// Its buttons: a title, whether it is the default, and what it sends.
    pub actions: Vec<(&'static str, bool, M)>,
}

/// The sign-in page: centred in the window, the name over what the machine
/// says (the status announced as it changes, the error as an alert) and
/// the buttons under it, the window's own push buttons.
pub fn welcome<M: Clone + 'static>(w: Welcome<M>) -> AnyView {
    let actions: Vec<AnyView> = w.actions.into_iter().map(|(t, default, m)| push_button(t, true, default, m)).collect();
    html::div()
        .class("ark-welcome")
        .child(
            html::div().class("ark-welcome-card").child((
                html::h1().class("ark-welcome-title").child(w.title),
                html::p().class("ark-caption ark-welcome-status").attr("role", "status").child(w.status),
                w.error
                    .map(|e| html::p().class("ark-problem ark-welcome-error").attr("role", "alert").child(e)),
                (!actions.is_empty()).then(|| html::div().class("ark-welcome-actions").child(actions)),
            )),
        )
        .into_any()
}
