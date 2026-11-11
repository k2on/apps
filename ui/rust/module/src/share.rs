//! Share's port: text (and a link) handed to the system's share sheet. An
//! update asks with [`share`], a [`ShareOp`] whose output is `()`, given
//! once the sheet is shown. Its implementations and its fake are the
//! module's (modules/share, `ark-share`).

use crate::{Op, Reply};

/// The system's share sheet.
pub trait Share: Send + Sync {
    fn share(&self, text: String, url: Option<String>);
}

/// What an update asks of the share sheet.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ShareOp {
    Share { text: String, url: Option<String> },
}

impl Op for ShareOp {
    type Module = dyn Share;
    type Output = ();
    const MODULE: &'static str = "share";
    fn run(&self, m: &dyn Share, reply: Reply<()>) {
        match self {
            ShareOp::Share { text, url } => m.share(text.clone(), url.clone()),
        }
        reply.done(())
    }
}

/// `text`, and `url` when there is one, handed to the share sheet.
pub fn share(text: impl Into<String>, url: Option<String>) -> ShareOp {
    ShareOp::Share { text: text.into(), url }
}
