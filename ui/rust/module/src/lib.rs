//! What every device module shares, the way Expo's modules core does it:
//! the plumbing, and no module. Each module is a package of its own under
//! `modules/<name>` (`ark-haptics`, `ark-clipboard`, `ark-share`,
//! `ark-camera`, `ark-media-player`, `ark-notifications`,
//! `ark-secure-store`): its trait, its calls, its fake, its desktop and web
//! implementations in Rust, its Swift and Kotlin ones beside the crate, and
//! a README of what it does on each platform and the permissions it needs
//! (ui/docs/app.md, The Device).
//!
//! What is here:
//!
//! - [`ModuleCall`] — one thing an update asks of the device, as data: a
//!   module's own call (a type implementing [`Call`]) behind one type the
//!   runtime carries, compares and prints. An app's `update` (`ark_app`)
//!   never calls a module itself: it returns a call, the runtime hands it
//!   to the [`Modules`] the shell registered, and the answer comes back.
//! - [`ModuleAnswer`] — what came of a call, in the few shapes every
//!   module answers in: done, a text, some bytes, unavailable, failed.
//!   `ark_app` turns it into the app's message (`Command::Ask`'s `then`, a
//!   plain function pointer), so an update is a pure function a test runs
//!   over the modules' fakes.
//! - [`ModuleReply`] — where a module that answers later (the camera, when the
//!   person is done) says what came of a call, once. Under `ffi` it is a
//!   UniFFI object the Swift and Kotlin implementations are handed.
//! - [`Modules`] — the registry a shell fills at startup, one trait object
//!   per module, looked up by the module's trait. A call to a module that
//!   is not registered (haptics on the desktop) is answered
//!   [`ModuleAnswer::Unavailable`].
//!
//! The UniFFI foreign-trait pattern: under its `ffi` feature each module
//! crate marks its trait `#[uniffi::export(with_foreign)]` (harken's
//! `PhoneSound` pattern), so Swift and Kotlin classes implement the
//! protocol or interface UniFFI writes, and an app's ffi crate takes them
//! (`Option<Arc<dyn Haptics>>`) and registers them here. Events a module
//! sends on its own (a player's position) go through a listener trait of
//! the module's, which the app's ffi crate implements and turns into
//! messages (ark-media-player's `MediaListener`).

use std::any::{Any, TypeId};
use std::collections::HashMap;
use std::fmt::{self, Debug};
use std::sync::{Arc, Mutex};

#[cfg(feature = "ffi")]
uniffi::setup_scaffolding!();

// * The answers

/// What came of a call.
#[derive(Clone, Debug, PartialEq)]
#[cfg_attr(feature = "ffi", derive(uniffi::Enum))]
pub enum ModuleAnswer {
    /// It was done (or, for a haptic, asked: nothing says whether it was felt).
    Done,
    /// A text: what a scan read, a secret kept. None when the person
    /// backed out or nothing was there.
    Text { value: Option<String> },
    /// Some bytes: a photo's JPEG. None when the person backed out.
    Bytes { value: Option<Vec<u8>> },
    /// The platform has no such module, or it is not registered.
    Unavailable,
    /// It failed, in the platform's words.
    Failed { why: String },
}

// * The calls

/// One module's call: what it asks, and how its module carries it out.
/// A module crate's calls are an enum of its own that implements this.
pub trait Call: Debug + PartialEq + Send + Sync + 'static {
    /// The module's trait object, as registered (`dyn Haptics`).
    type Module: ?Sized + Send + Sync + 'static;
    /// The module's name, for what is printed.
    const MODULE: &'static str;
    /// `self` carried out by `module`; what came of it to `reply`, now or
    /// later, once.
    fn run(&self, module: &Self::Module, reply: Arc<ModuleReply>);
}

/// A call with its type erased: what [`ModuleCall`] holds.
trait Erased: Send + Sync {
    fn run(&self, modules: &Modules, reply: Arc<ModuleReply>);
    fn module(&self) -> &'static str;
    fn debug(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result;
    fn same(&self, other: &dyn Erased) -> bool;
    fn as_any(&self) -> &dyn Any;
}

impl<C: Call> Erased for C {
    fn run(&self, modules: &Modules, reply: Arc<ModuleReply>) {
        match modules.get::<C::Module>() {
            Some(m) => Call::run(self, &*m, reply),
            None => reply.answer(ModuleAnswer::Unavailable),
        }
    }
    fn module(&self) -> &'static str {
        C::MODULE
    }
    fn debug(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        Debug::fmt(self, f)
    }
    fn same(&self, other: &dyn Erased) -> bool {
        other.as_any().downcast_ref::<C>() == Some(self)
    }
    fn as_any(&self) -> &dyn Any {
        self
    }
}

/// One thing an update asks of the device: any module's call.
#[derive(Clone)]
pub struct ModuleCall(Arc<dyn Erased>);

impl ModuleCall {
    pub fn new(call: impl Call) -> ModuleCall {
        ModuleCall(Arc::new(call))
    }

    /// The module it is for (`"haptics"`).
    pub fn module(&self) -> &'static str {
        self.0.module()
    }

    /// The call, as its module's own type, when it is one.
    pub fn get<C: Call>(&self) -> Option<&C> {
        self.0.as_any().downcast_ref()
    }
}

impl<C: Call> From<C> for ModuleCall {
    fn from(c: C) -> ModuleCall {
        ModuleCall::new(c)
    }
}

impl Debug for ModuleCall {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.debug(f)
    }
}

impl PartialEq for ModuleCall {
    fn eq(&self, other: &ModuleCall) -> bool {
        self.0.same(&*other.0)
    }
}

// * Answering later

/// What a reply hands its answer to.
type Answering = Box<dyn FnOnce(ModuleAnswer) + Send>;

/// Where a module says what came of a call, once.
#[cfg_attr(feature = "ffi", derive(uniffi::Object))]
pub struct ModuleReply {
    to: Mutex<Option<Answering>>,
}

impl ModuleReply {
    /// A reply that hands its answer to `to`.
    pub fn new(to: impl FnOnce(ModuleAnswer) + Send + 'static) -> Arc<ModuleReply> {
        Arc::new(ModuleReply {
            to: Mutex::new(Some(Box::new(to))),
        })
    }
}

#[cfg_attr(feature = "ffi", uniffi::export)]
impl ModuleReply {
    /// What came of the call; only the first answer is heard.
    pub fn answer(&self, answer: ModuleAnswer) {
        let to = self.to.lock().unwrap_or_else(|e| e.into_inner()).take();
        if let Some(to) = to {
            to(answer)
        }
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

    /// `call` carried out by the module registered for it; what came of it
    /// handed to `reply`, at once or (the camera) when the person is done.
    pub fn call(&self, call: ModuleCall, reply: impl FnOnce(ModuleAnswer) + Send + 'static) {
        call.0.run(self, ModuleReply::new(reply))
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

    impl Call for Say {
        type Module = dyn Echo;
        const MODULE: &'static str = "echo";
        fn run(&self, m: &dyn Echo, reply: Arc<ModuleReply>) {
            reply.answer(ModuleAnswer::Text {
                value: Some(m.echo(self.0.clone())),
            })
        }
    }

    fn heard(modules: &Modules, call: ModuleCall) -> Vec<ModuleAnswer> {
        let got = Arc::new(Mutex::new(vec![]));
        let g = got.clone();
        modules.call(call, move |a| g.lock().unwrap().push(a));
        let v = got.lock().unwrap().clone();
        v
    }

    #[test]
    fn a_call_reaches_its_module_and_answers() {
        let mut m = Modules::new();
        m.register::<dyn Echo>(Arc::new(Loud));
        assert_eq!(heard(&m, Say("hi".into()).into()), vec![ModuleAnswer::Text { value: Some("HI".into()) }]);
    }

    #[test]
    fn a_missing_module_is_unavailable() {
        assert_eq!(heard(&Modules::new(), Say("hi".into()).into()), vec![ModuleAnswer::Unavailable]);
        let mut m = Modules::new();
        m.register::<dyn Echo>(Arc::new(Loud)).set::<dyn Echo>(None);
        assert_eq!(heard(&m, Say("hi".into()).into()), vec![ModuleAnswer::Unavailable]);
    }

    #[test]
    fn calls_compare_and_print_as_their_own() {
        let a: ModuleCall = Say("a".into()).into();
        assert_eq!(a, Say("a".into()).into());
        assert_ne!(a, Say("b".into()).into());
        assert_eq!(format!("{a:?}"), "Say(\"a\")");
        assert_eq!(a.module(), "echo");
        assert_eq!(a.get::<Say>(), Some(&Say("a".into())));
    }

    #[test]
    fn a_reply_is_heard_once() {
        let got = Arc::new(Mutex::new(vec![]));
        let g = got.clone();
        let r = ModuleReply::new(move |a| g.lock().unwrap().push(a));
        r.answer(ModuleAnswer::Done);
        r.answer(ModuleAnswer::Unavailable);
        assert_eq!(*got.lock().unwrap(), vec![ModuleAnswer::Done]);
    }
}
