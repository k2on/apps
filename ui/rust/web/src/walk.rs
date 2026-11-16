//! A screenshot walk's steps, taken inside the page (ui/docs/shots.md, "In
//! the App"), as the desktop's window and the phones take theirs: a
//! screenshot's page (`?shots=1`) puts `window.arkShots` on the page, and
//! the driver (.github/screens/web-shots.js) hands it each step of the wide
//! walk (shots.json's `wide`) that names an element by its id. The step is
//! taken against the views on the page now (`ark_app::shots::windowed`:
//! the shell, each screen it hosts, what is over it, the first run's page
//! while it shows) by sending the message the element it names sends, as
//! a click on it does; the page draws again after. Keys, waits and a click
//! by a text are the driver's own, as a hand's.
//!
//! - `arkShots.step(kind, id, item)`: `kind` is `tap`, `text`, `press`
//!   (with the menu's `item`), `await` or `type` (`id` the text). Answers
//!   `done`, `notyet` and the tree under it (nothing names it yet: asked
//!   again once the page has changed), or `failed` and why.
//! - `arkShots.tree()`: what a step can name now, a line each (written
//!   beside each shot as `<id>.tree.txt`).

use std::cell::RefCell;
use std::rc::Rc;

use ark_app::shots::{self, Screen, Step, Took};

use crate::host::{HostMsg, Msg, WebHost, WebHosted};

thread_local! {
    /// The field a step tapped last, which a typed step types into.
    static FOCUS: RefCell<Option<String>> = const { RefCell::new(None) };
}

impl<W: WebHosted> WebHost<W> {
    /// What a step can reach on the page now, each element's message put
    /// in `out`.
    fn shots_targets(&self, out: Rc<RefCell<Vec<HostMsg<Msg<W>>>>>) -> Vec<shots::Target> {
        if let Some(live) = self.live.as_ref().filter(|l| !l.running() || l.first_run.page().intro.is_some()) {
            let view = ark_app::welcome::welcome::view(&live.first_run.page());
            let send = Rc::new(move |m| out.borrow_mut().push(HostMsg::Welcome(m)));
            return shots::compose(vec![Screen {
                name: "welcome".into(),
                nodes: shots::nodes(&view, send),
            }]);
        }
        let send = Rc::new(move |m: Msg<W>| out.borrow_mut().push(HostMsg::App(m)));
        shots::windowed(self.app(), &self.seen(), self.now_ms(), send)
    }
}

/// A step as the driver hands it: its kind, the id it names (or the text it
/// types) and a menu's item.
fn step_of(kind: &str, id: String, item: String) -> Option<Step> {
    Some(match kind {
        "tap" => Step::Tap { id, text: false },
        "text" => Step::Tap { id, text: true },
        "press" => Step::Press { id, item },
        "await" => Step::Await { id },
        "type" => Step::Type { text: id },
        _ => return None,
    })
}

/// One step taken on `app`'s page: what came of it, as `arkShots.step`
/// answers.
fn take<W: WebHosted>(app: &Rc<RefCell<WebHost<W>>>, kind: &str, id: String, item: String) -> String {
    let Some(step) = step_of(kind, id, item) else {
        return format!("failed\nno such step: {kind}");
    };
    let out = Rc::new(RefCell::new(vec![]));
    let targets = app.borrow().shots_targets(out.clone());
    let took = FOCUS.with(|f| shots::take(&step, &targets, &mut f.borrow_mut()));
    match took {
        Took::Done => {
            let sent = std::mem::take(&mut *out.borrow_mut());
            drop(targets);
            for m in sent {
                crate::send(m);
            }
            "done".into()
        }
        Took::NotYet => format!("notyet\n{}", shots::tree(&targets)),
        Took::Failed(why) => format!("failed\n{why}"),
    }
}

/// `window.arkShots`, on a screenshot's page alone.
pub(crate) fn install<W: WebHosted>(app: &Rc<RefCell<WebHost<W>>>) {
    if !cfg!(target_arch = "wasm32") || !crate::shots() {
        return;
    }
    use wasm_bindgen::closure::Closure;
    use wasm_bindgen::JsCast;
    use web_sys::js_sys::{Object, Reflect};
    let Some(w) = web_sys::window() else { return };
    let api = Object::new();
    let step = {
        let app = app.clone();
        Closure::<dyn Fn(String, String, String) -> String>::new(move |kind: String, id: String, item: String| take(&app, &kind, id, item))
    };
    let tree = {
        let app = app.clone();
        Closure::<dyn Fn() -> String>::new(move || shots::tree(&app.borrow().shots_targets(Rc::new(RefCell::new(vec![])))))
    };
    let _ = Reflect::set(&api, &"step".into(), step.as_ref().unchecked_ref());
    let _ = Reflect::set(&api, &"tree".into(), tree.as_ref().unchecked_ref());
    let _ = Reflect::set(&w, &"arkShots".into(), &api);
    step.forget();
    tree.forget();
}
