//! A screenshot walk's steps, taken inside the app (ui/docs/shots.md, "In
//! the App"): the phone's shots mode hands each step here, as shots.json
//! writes it, and it is taken against the views on the screen now
//! (`ark_app::shots`) — the first run's pages while they show, the app's
//! runtimes' after — by sending the message the element it names sends,
//! through the same runtime's `send` a finger's tap goes through.
//!
//! And the walk's sign-in. A dev server with its own user (`ARK_DEV_USER`)
//! answers its sign-in page with the code at once; on a phone that page is
//! opened in the system's browser sheet, which is not the app's and which
//! nothing inside the app can reach. So a walk's launch ([`Launch::walk`],
//! a plan the shell found in the app's own directory on a simulator or an
//! emulator) reads that answer itself, as the desktop does, and takes the
//! code it carries. It cannot weaken a real sign-in: a server that is not
//! a dev server answers with a page for a person (or a provider's), never
//! with the app's own `<scheme>://auth?code=`, and then the sheet opens as
//! ever. And a build for people never does it: only a simulator's build
//! (`target_abi = "sim"`) or Android's, whose shell says `walk` only on an
//! emulator ([`STRAIGHT`]).
//!
//! [`Launch::walk`]: super::Launch::walk

use std::rc::Rc;

use ark_app::shots::{self, Role, Step, Took};

use super::ArkHost;

/// Whether this build may take a walk's sign-in without the browser: an
/// iOS simulator's, or Android's (on an emulator alone: its shell's
/// `walk`); never an iPhone's, an iPad's or a Mac's for people.
const STRAIGHT: bool = cfg!(any(target_abi = "sim", target_os = "android", test));

/// What came of a walk's step.
#[derive(Clone, Debug, PartialEq, Eq, uniffi::Enum)]
pub enum ShotsTook {
    /// Done: its message sent. The screen is waited for, then the next.
    Done,
    /// Nothing on the screen says what it names, yet: asked again once the
    /// screen has changed, until the walk's limit (then `labels`, what was
    /// there, is why it failed).
    NotYet { labels: String },
    /// It cannot be done.
    Failed { why: String },
}

/// A step as shots.json writes it (.github/screens/apps.md).
pub(crate) fn step_of(json: &str) -> Result<Step, String> {
    let v: serde_json::Value = serde_json::from_str(json).map_err(|e| format!("{json}: {e}"))?;
    let s = |k: &str| v.get(k).and_then(|x| x.as_str()).map(String::from);
    if let Some(label) = s("tap") {
        let role = match s("role") {
            Some(r) => Some(Role::of(&r).ok_or_else(|| format!("no role {r}: tab, button, field, row, item or text"))?),
            None => None,
        };
        let bottom = s("where").as_deref() == Some("bottom");
        return Ok(Step::Tap { label, role, bottom });
    }
    if let Some(text) = s("type") {
        return Ok(Step::Type { text });
    }
    if let Some(label) = s("press") {
        let item = s("item").ok_or_else(|| format!("a press names the item of its menu: {json}"))?;
        return Ok(Step::Press { label, item });
    }
    if let Some(label) = s("await") {
        return Ok(Step::Await { label });
    }
    Err(format!("no such step: {json}"))
}

impl ArkHost {
    /// What a step can reach now: the first run's page while one shows,
    /// else the app's runtimes' views.
    fn shots_targets(&self) -> Vec<shots::Target> {
        if self.shows_app() {
            return shots::compose(self.app.shots());
        }
        let page = self.with_run(|r| r.page());
        let view = ark_app::welcome::welcome::view(&page);
        let me = self.me.clone();
        let send = Rc::new(move |m| {
            if let Some(h) = me.upgrade() {
                h.press(m)
            }
        });
        shots::compose(vec![shots::Screen {
            name: "welcome".into(),
            nodes: shots::nodes(&view, send),
        }])
    }

    /// A walk's sign-in at `url`, taken straight back when its server is a
    /// dev server that sends it so, else the browser's: whether it was a
    /// walk's to take.
    pub(super) fn straight(&self, url: &str) -> bool {
        if !(STRAIGHT && self.launch.walk) {
            return false;
        }
        let scheme = self.app.info().scheme;
        let url = url.to_string();
        self.off(move |h| match ark_auth::client::straight_back(&url, &scheme) {
            Some(back) => h.returned(Some(back), None),
            None => {
                let shell = h.lock().shell.clone();
                if let Some(shell) = shell {
                    shell.browse(url)
                }
            }
        });
        true
    }
}

#[uniffi::export]
impl ArkHost {
    /// A walk's step (one of shots.json's, as JSON) taken against what is
    /// on the screen now.
    pub fn shots_step(&self, step: String) -> ShotsTook {
        let step = match step_of(&step) {
            Ok(s) => s,
            Err(why) => return ShotsTook::Failed { why },
        };
        let targets = self.shots_targets();
        let mut focus = self.lock().focus.clone();
        let took = shots::take(&step, &targets, &mut focus);
        self.lock().focus = focus;
        match took {
            Took::Done => ShotsTook::Done,
            Took::NotYet => ShotsTook::NotYet {
                labels: shots::tree(&targets),
            },
            Took::Failed(why) => ShotsTook::Failed { why },
        }
    }

    /// What is on the screen now, as a step names it: a line each, written
    /// beside each shot (`<id>.tree.txt`).
    pub fn shots_tree(&self) -> String {
        shots::tree(&self.shots_targets())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Each step as shots.json writes it. Falsified by a role that is not
    /// one read as none, or a press with no item taken.
    #[test]
    fn steps_read_as_written() {
        assert_eq!(
            step_of(r#"{"tap": "Budget", "role": "tab"}"#),
            Ok(Step::Tap {
                label: "Budget".into(),
                role: Some(Role::Tab),
                bottom: false
            })
        );
        assert_eq!(
            step_of(r#"{"tap": "Auto-Assign", "where": "bottom"}"#),
            Ok(Step::Tap {
                label: "Auto-Assign".into(),
                role: None,
                bottom: true
            })
        );
        assert_eq!(
            step_of(r#"{"type": "http://10.0.2.2:8788"}"#),
            Ok(Step::Type {
                text: "http://10.0.2.2:8788".into()
            })
        );
        assert_eq!(step_of(r#"{"await": "Budget"}"#), Ok(Step::Await { label: "Budget".into() }));
        assert!(step_of(r#"{"tap": "Budget", "role": "tabs"}"#).is_err());
        assert!(step_of(r#"{"press": "Groceries"}"#).is_err());
        assert!(step_of(r#"{"wait": 2}"#).is_err());
    }
}
