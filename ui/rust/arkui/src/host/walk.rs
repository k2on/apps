//! A screenshot walk's steps, taken inside the app (ui/docs/shots.md, "In
//! the App"), as the phones take theirs (ark-ffi's `shots`): arkui-shots
//! hands each step of a shot here, and it is taken against the views on
//! the screen now (`ark_app::shots`) — the first run's page while it
//! shows, else the window's shell, each screen it hosts, what is over it —
//! by sending the message the element it names sends, through the window's
//! own update. A step names an element by its id (`tab:budget`,
//! `budget.row:dining-out`, `shell.new-transaction`), the same ids the
//! phones' walks and the browser's `data-ark-id` use; a shot's
//! `<id>.tree.txt` lists them. Keys (`{"key": "j"}`) and waits are the
//! driver's own, as a hand's.

use std::cell::RefCell;
use std::rc::Rc;

use ark_app::shots::{self, Screen, Step, Target, Took};
use ark_app::window::Windowed;
use arkui_shots::Walked;
use serde_json::Value;

use super::{Desktop, DesktopHost, Msg};

type AppMsg<W> = <<W as Desktop>::App as Windowed>::Msg;

/// A step as shots.json writes it (.github/screens/apps.md); none for a
/// step that is the driver's (a key, a wait).
pub fn step(v: &Value) -> Option<Step> {
    let s = |k: &str| v.get(k).and_then(|x| x.as_str()).map(String::from);
    if let Some(id) = s("tap") {
        return Some(Step::Tap { id, text: false });
    }
    if let Some(id) = s("text") {
        return Some(Step::Tap { id, text: true });
    }
    if let (Some(id), Some(item)) = (s("press"), s("item")) {
        return Some(Step::Press { id, item });
    }
    if let Some(id) = s("await") {
        return Some(Step::Await { id });
    }
    // Typed into the field a step tapped: a walk's, while one has the keys.
    if let Some(text) = s("type").filter(|_| v.get("into").and_then(Value::as_str) != Some("keys")) {
        return Some(Step::Type { text });
    }
    None
}

impl<W: Desktop> DesktopHost<W>
where
    AppMsg<W>: Send,
{
    /// What a step can reach now, each element's message put in `out`.
    fn targets(&self, out: Rc<RefCell<Vec<Msg<AppMsg<W>>>>>) -> Vec<Target> {
        if let Some(live) = self.live.as_ref().filter(|l| !l.running() || l.first_run.page().intro.is_some()) {
            let view = ark_app::welcome::welcome::view(&live.first_run.page());
            let send = Rc::new(move |m| out.borrow_mut().push(Msg::Welcome(m)));
            return shots::compose(vec![Screen {
                name: "welcome".into(),
                nodes: shots::nodes(&view, send),
            }]);
        }
        let send = Rc::new(move |m: AppMsg<W>| out.borrow_mut().push(Msg::App(m)));
        shots::windowed(self.app(), &self.seen(), self.now_ms(), send)
    }

    /// One of a shot's steps taken against what is on the screen now: the
    /// messages it sends, or that it is not this walk's (a key), not there
    /// yet, or cannot be done.
    pub fn walk(&self, v: &Value) -> Walked<Msg<AppMsg<W>>> {
        let Some(step) = step(v) else { return Walked::Not };
        let out = Rc::new(RefCell::new(vec![]));
        let targets = self.targets(out.clone());
        let mut focus = self.walk_focus.borrow().clone();
        let took = shots::take(&step, &targets, &mut focus);
        *self.walk_focus.borrow_mut() = focus;
        match took {
            Took::Done => Walked::Send(std::mem::take(&mut *out.borrow_mut())),
            Took::NotYet => Walked::NotYet(shots::tree(&targets)),
            Took::Failed(why) => Walked::Failed(why),
        }
    }

    /// What is on the screen now, as a step names it: written beside each
    /// shot (`<id>.tree.txt`).
    pub fn walk_tree(&self) -> String {
        shots::tree(&self.targets(Rc::new(RefCell::new(vec![]))))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Each step as shots.json writes it; a key is the driver's.
    #[test]
    fn steps_read_as_written() {
        let v = |s: &str| serde_json::from_str::<Value>(s).unwrap();
        assert_eq!(
            step(&v(r#"{"tap": "tab:budget"}"#)),
            Some(Step::Tap {
                id: "tab:budget".into(),
                text: false
            })
        );
        assert_eq!(step(&v(r#"{"await": "tab:insights"}"#)), Some(Step::Await { id: "tab:insights".into() }));
        assert_eq!(step(&v(r#"{"key": "j"}"#)), None);
        assert_eq!(step(&v(r#"{"type": "Tar", "into": "keys"}"#)), None);
    }
}
