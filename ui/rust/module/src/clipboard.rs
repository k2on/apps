//! The clipboard's port: text put where the person can paste it. An update
//! asks with [`copy`], a [`ClipboardOp`] whose output is `()`. Its
//! implementations (the phones', an iced window's queue, the browser's),
//! and its fake, are the module's (modules/clipboard, `ark-clipboard`).

use crate::{Op, Reply};

/// Text put on the clipboard.
pub trait Clipboard: Send + Sync {
    fn copy_text(&self, text: String);
}

/// What an update asks of the clipboard.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ClipboardOp {
    Copy(String),
}

impl Op for ClipboardOp {
    type Module = dyn Clipboard;
    type Output = ();
    const MODULE: &'static str = "clipboard";
    fn run(&self, m: &dyn Clipboard, reply: Reply<()>) {
        match self {
            ClipboardOp::Copy(t) => m.copy_text(t.clone()),
        }
        reply.done(())
    }
}

/// `text` put on the clipboard.
pub fn copy(text: impl Into<String>) -> ClipboardOp {
    ClipboardOp::Copy(text.into())
}
