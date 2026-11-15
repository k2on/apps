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

use ark_app::shots::{self, Node, Screen, Step, Target, Took};
use ark_app::view::View;
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

/// One screen's message, as its own view sends it: the app's window
/// message (`Windowed::Msg`, an enum of its screens' messages) with that
/// one level of wrapping left out of its `Debug`, so an element's id is
/// worked out from the screen's message (`shell.inspector:category-details`,
/// `transactions.show:all`), as the phones' walks and the browser's
/// `data-ark-id` have it, never from the window's (`shell.shell:…`).
#[derive(Clone)]
struct Own<M>(M);

impl<M: std::fmt::Debug> std::fmt::Debug for Own<M> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let s = format!("{:?}", self.0);
        f.write_str(unwrapped(&s))
    }
}

/// `Shell(Inspector { .. })` as `Inspector { .. }`; anything not one
/// variant around one value as it is.
fn unwrapped(s: &str) -> &str {
    match s.find('(') {
        Some(i) if s.ends_with(')') && i > 0 && s[..i].chars().all(|c| c.is_alphanumeric() || c == '_') => &s[i + 1..s.len() - 1],
        _ => s,
    }
}

/// The names `nodes` host, shown.
fn hosted(nodes: &[Node]) -> Vec<String> {
    nodes
        .iter()
        .filter_map(|n| match n {
            Node::Hosted { name, shown: true } => Some(name.clone()),
            _ => None,
        })
        .collect()
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
        let seen = self.seen();
        let now = self.now_ms();
        let app = self.app();
        let send: Rc<dyn Fn(Own<AppMsg<W>>)> = Rc::new(move |m: Own<AppMsg<W>>| out.borrow_mut().push(Msg::App(m.0)));
        let window: View<Own<AppMsg<W>>> = match app.settings(&seen, now) {
            Some((panes, _)) => panes,
            None => app.window(&seen, now),
        }
        .map(Own);
        let mut screens = vec![Screen {
            name: <W::App as Windowed>::WINDOW.into(),
            nodes: shots::nodes(&window, send.clone()),
        }];
        // Each name hosted, as its screen (a hosted name's last part,
        // `register` of `accounts.register`), and what it hosts in turn.
        let mut names = hosted(&screens[0].nodes);
        let mut done: Vec<String> = vec![];
        while let Some(name) = names.pop() {
            if done.contains(&name) {
                continue;
            }
            let nodes = shots::nodes(&app.hosted(&name, &seen, now).map(Own), send.clone());
            names.extend(hosted(&nodes));
            screens.push(Screen {
                name: name.rsplit('.').next().unwrap_or(&name).to_string(),
                nodes,
            });
            done.push(name);
        }
        screens.push(Screen {
            name: <W::App as Windowed>::OVER.into(),
            nodes: shots::nodes(&app.over(&seen, now).map(Own), send),
        });
        shots::compose(screens)
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

    /// An id is the screen's message's, not the window's around it.
    /// Falsified by unwrapping a message that is no one-value variant.
    #[test]
    fn the_windows_wrapping_is_left_out() {
        assert_eq!(unwrapped("Shell(Inspector { on: false })"), "Inspector { on: false }");
        assert_eq!(unwrapped("Transactions(Show { all: true })"), "Show { all: true }");
        assert_eq!(unwrapped("Inspector { on: false }"), "Inspector { on: false }");
        assert_eq!(unwrapped("Done"), "Done");
    }
}
