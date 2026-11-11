//! harken as an app over its domain (ui/docs/app.md, Layers): what is
//! neither the domain's rules nor a shell's drawing. Today that is where
//! harken can be ([`routes`]), which every shell spells the same way; the
//! phones read it through harken/ffi, which writes its UniFFI twin.

pub mod routes;

pub use routes::Route;
