//! What every device module shares: the plumbing, and no module. Each
//! module is a package of its own under `modules/<name>` (`ark-haptics`,
//! `ark-clipboard`, `ark-share`, `ark-camera`, `ark-media-player`,
//! `ark-notifications`, `ark-secure-store`, `ark-scanner`, `ark-dpc`): its trait, its operations, its
//! fake, its desktop and web implementations in Rust, its Swift and Kotlin
//! ones beside the crate, and a README of what it does on each platform and
//! the permissions it needs (ui/docs/app.md, Effects and Ports).
//!
//! What is here:
//!
//! - [`Op`] — one thing an update asks of the device, as data, with the
//!   type of what comes of it: a haptic's [`Op::Output`] is `()`, a QR
//!   scan's the code read, a photo's its JPEG. Each module declares its
//!   operations (an enum or a struct each) and their outputs, and lists
//!   them in its `module.toml`'s `[ops]` (ui/prebuild.py checks the two
//!   agree). An app's `update` (`ark_app`) never runs one: it returns it,
//!   the runtime hands it to the [`Modules`] the shell registered, and
//!   what came of it comes back typed.
//! - [`ModuleError`] — why an operation gave nothing: the module is not
//!   there, or it failed in the platform's words.
//! - [`Reply`] — where a module that answers later (the camera, when the
//!   person is done) says what came of an operation, once.
//! - [`Modules`] — the registry a shell fills at startup, one trait object
//!   per module, looked up by the module's trait. An operation of a module
//!   that is not registered (haptics on the desktop) is answered
//!   [`ModuleError::Unavailable`].
//!
//! - The ports an app's update asks through ([`haptics`], [`clipboard`],
//!   [`share`], [`scanner`], [`media_player`], [`secure_store`], [`dpc`]): each module's trait, its operations and their outputs,
//!   here in the runtime, so an app reaches a module only through these
//!   typed operations and never depends on the module's crate (layers.toml).
//!   The module's crate implements the port (its fake, the desktop's, the
//!   browser's, the phones').
//!
//! This crate is pure and has no UniFFI. Under its `ffi` feature each
//! module crate declares a foreign trait of the same name
//! (`ark_haptics::phone::Haptics`, `#[uniffi::export(with_foreign)]`), so
//! Swift and Kotlin classes implement the protocol or interface UniFFI
//! writes, and an app's bridge takes them and registers each as the port
//! (`ark_haptics::phone::port`). A module that
//! answers later hands its foreign implementation a typed reply object of
//! its own (the camera's `ScanReply` and `PhotoReply`), so what Swift and
//! Kotlin answer is the operation's output, not a shape shared by all.
//! Events a module sends on its own (a player's position) go through a
//! listener trait of its port's ([`media_player::MediaListener`]).

use std::any::{Any, TypeId};
use std::collections::HashMap;
use std::fmt::{self, Debug};
use std::sync::{Arc, Mutex};

pub mod clipboard;
pub mod dpc;
pub mod haptics;
pub mod media_player;
pub mod scanner;
pub mod secure_store;
pub mod share;

// * What came of an operation

/// Why an operation gave nothing.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ModuleError {
    /// The platform has no such module, or it is not registered.
    Unavailable,
    /// It failed, in the platform's words.
    Failed { why: String },
}

impl fmt::Display for ModuleError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ModuleError::Unavailable => f.write_str("not available here"),
            ModuleError::Failed { why } => f.write_str(why),
        }
    }
}

/// What came of an operation whose output is `T`.
pub type Answer<T> = Result<T, ModuleError>;

// * The operations

/// One module's operation: what it asks, what comes of it, and how its
/// module carries it out.
pub trait Op: Debug + PartialEq + Send + Sync + 'static {
    /// The module's trait object, as registered (`dyn Haptics`).
    type Module: ?Sized + Send + Sync + 'static;
    /// What comes of it: `()` for a haptic, the code a scan read.
    type Output: Debug + Send + 'static;
    /// The module's name, for what is printed.
    const MODULE: &'static str;
    /// `self` carried out by `module`; what came of it to `reply`, now or
    /// later, once.
    fn run(&self, module: &Self::Module, reply: Reply<Self::Output>);
}

// * Answering later

/// What a reply hands its answer to.
type Answering<T> = Box<dyn FnOnce(Answer<T>) + Send>;

/// Where a module says what came of an operation, once: only the first
/// answer is heard. A clone is the same reply.
pub struct Reply<T> {
    to: Arc<Mutex<Option<Answering<T>>>>,
}

impl<T> Clone for Reply<T> {
    fn clone(&self) -> Self {
        Reply { to: self.to.clone() }
    }
}

impl<T> Reply<T> {
    /// A reply that hands its answer to `to`.
    pub fn new(to: impl FnOnce(Answer<T>) + Send + 'static) -> Reply<T> {
        Reply {
            to: Arc::new(Mutex::new(Some(Box::new(to)))),
        }
    }

    /// What came of the operation.
    pub fn answer(&self, answer: Answer<T>) {
        let to = self.to.lock().unwrap_or_else(|e| e.into_inner()).take();
        if let Some(to) = to {
            to(answer)
        }
    }

    /// It was done, and this came of it.
    pub fn done(&self, output: T) {
        self.answer(Ok(output))
    }

    /// It failed, in the platform's words.
    pub fn failed(&self, why: impl Into<String>) {
        self.answer(Err(ModuleError::Failed { why: why.into() }))
    }
}

// * The registry

/// The modules a shell registered at startup, by trait; any may be missing.
#[derive(Clone, Default)]
pub struct Modules {
    by: HashMap<TypeId, Arc<dyn Any + Send + Sync>>,
}

impl Modules {
    pub fn new() -> Modules {
        Modules::default()
    }

    /// `module` registered as the one for its trait (`M` is `dyn Haptics`),
    /// in place of any before.
    pub fn register<M: ?Sized + Send + Sync + 'static>(&mut self, module: Arc<M>) -> &mut Modules {
        self.by.insert(TypeId::of::<M>(), Arc::new(module));
        self
    }

    /// `module` registered when there is one, and the trait's registration
    /// dropped when there is none (what a phone hands over: `Option`s).
    pub fn set<M: ?Sized + Send + Sync + 'static>(&mut self, module: Option<Arc<M>>) -> &mut Modules {
        match module {
            Some(m) => self.register(m),
            None => {
                self.by.remove(&TypeId::of::<M>());
                self
            }
        }
    }

    /// The module registered for the trait `M`.
    pub fn get<M: ?Sized + Send + Sync + 'static>(&self) -> Option<Arc<M>> {
        self.by.get(&TypeId::of::<M>())?.downcast_ref::<Arc<M>>().cloned()
    }

    /// `op` carried out by the module registered for it; what came of it
    /// handed to `answer`, at once or (the camera) when the person is done.
    pub fn run<O: Op>(&self, op: &O, answer: impl FnOnce(Answer<O::Output>) + Send + 'static) {
        let reply = Reply::new(answer);
        match self.get::<O::Module>() {
            Some(m) => op.run(&*m, reply),
            None => reply.answer(Err(ModuleError::Unavailable)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    trait Echo: Send + Sync {
        fn echo(&self, s: String) -> String;
    }

    struct Loud;
    impl Echo for Loud {
        fn echo(&self, s: String) -> String {
            s.to_uppercase()
        }
    }

    #[derive(Debug, PartialEq)]
    struct Say(String);

    impl Op for Say {
        type Module = dyn Echo;
        type Output = String;
        const MODULE: &'static str = "echo";
        fn run(&self, m: &dyn Echo, reply: Reply<String>) {
            reply.done(m.echo(self.0.clone()))
        }
    }

    fn heard(modules: &Modules, op: Say) -> Vec<Answer<String>> {
        let got = Arc::new(Mutex::new(vec![]));
        let g = got.clone();
        modules.run(&op, move |a| g.lock().unwrap().push(a));
        let v = got.lock().unwrap().clone();
        v
    }

    #[test]
    fn an_op_reaches_its_module_and_answers_its_output() {
        let mut m = Modules::new();
        m.register::<dyn Echo>(Arc::new(Loud));
        assert_eq!(heard(&m, Say("hi".into())), vec![Ok("HI".to_string())]);
    }

    #[test]
    fn a_missing_module_is_unavailable() {
        assert_eq!(heard(&Modules::new(), Say("hi".into())), vec![Err(ModuleError::Unavailable)]);
        let mut m = Modules::new();
        m.register::<dyn Echo>(Arc::new(Loud)).set::<dyn Echo>(None);
        assert_eq!(heard(&m, Say("hi".into())), vec![Err(ModuleError::Unavailable)]);
    }

    #[test]
    fn a_reply_is_heard_once() {
        let got = Arc::new(Mutex::new(vec![]));
        let g = got.clone();
        let r = Reply::<()>::new(move |a| g.lock().unwrap().push(a));
        r.clone().done(());
        r.failed("late");
        assert_eq!(*got.lock().unwrap(), vec![Ok(())]);
    }
}
