//! The page on its server: a replica of the household's budget, signed in
//! as a person, synced over the server's socket — what every native client
//! is, in the page (ui/docs/app.md, "The Browser's Modes").
//!
//! **Signing in** is the machine every app's shell walks
//! (`ark_auth::onboarding`), as the phones and the desktop walk it: the
//! server is whoever served the page, a load with nobody signed in goes to
//! the server's sign-in by itself (once a load), comes back to its own
//! address with a one-minute code, and trades it at `/auth/session`. The
//! session's token is then the server's `HttpOnly` cookie, which no script
//! on the page can read; what the page remembers between loads
//! (`ark_peer::Logins`, the page's `localStorage` under `money.logins`) is
//! the login without it — who, which session, until when. The socket says
//! nothing as its first message (`ark_auth::COOKIE_HELLO`), and the server
//! takes the cookie. A session the server turns away (signed out
//! elsewhere, expired) is forgotten and signed in again, once.
//!
//! **The replica** is an `ark_peer::Peer` over money's domain compiled to
//! wasm, dialling the socket whenever it has a login (`Peer::connect`,
//! the page's own `WebSocket`): the server's changes arrive as they are
//! made, and this page's are authored through the domain's mutators
//! (`Peer::mutate_bytes`) and shown at once, optimistically, as on every
//! device. A connection that drops is dialled again — at once when it had
//! been up, with backoff when a dial fails — and what was authored while
//! it was down waits as the peer's pending intents and is pushed again
//! when it is back.
//!
//! **In memory.** The replica's disk is `ark_peer::Memory`: a load fills
//! from the server, and pending intents live as long as the page does — a
//! change authored offline and the page closed before the server is back
//! is lost. ark-peer's `Disk` is synchronous (a write is on disk before
//! anything said of it is delivered) and the browser's IndexedDB is not,
//! so it is no `Disk` without a write-behind that breaks that promise;
//! `localStorage` (`ark_peer::disk::Local`) is one, but holds a few
//! megabytes, short of a household's years. Not done: a durable replica.

#[cfg(target_arch = "wasm32")]
use std::rc::Rc;

use ark_auth::onboarding::{AppInfo, Effect, Event, Onboarding, Phase, Remembered, Step};
use ark_peer::{Config, Logins, Memory, Peer};

use crate::{MoneyWeb, Msg};

/// Who money is, to the sign-in machine.
pub fn app() -> AppInfo {
    AppInfo {
        name: "Money".into(),
        scheme: "money".into(),
        demo: false,
        what: "your budget".into(),
    }
}

/// What the page holds on its server beside the replica.
pub struct Live {
    /// The server's base URL: the page's own origin.
    pub server: String,
    pub onboarding: Onboarding,
    /// The logins this browser remembers, the token not among them.
    pub logins: Logins,
    /// Who is signed in, once the session is open.
    pub login: Option<ark_auth::Login>,
    /// The timer that pumps the peer at its next deadline (a redial, a
    /// job), while one is set.
    timer: Option<i32>,
}

impl Live {
    /// Whether the app is shown: the session open, or on its way and the
    /// replica already holding a budget.
    pub fn running(&self) -> bool {
        matches!(self.onboarding.step(), Step::Running { .. })
    }
}

/// A replica of money's domain in memory, dialling `server`'s socket once
/// it has a login.
pub fn replica(server: &str) -> Peer {
    let mut peer = Peer::open_as(Config::of(money_core::shared()), Box::new(Memory::default()), None);
    // A page draws on the thread it takes frames in on: a fill is taken
    // between frames, a few milliseconds a pump.
    peer.link.budget = Some(8);
    peer.connect(&ark_auth::socket_url(server));
    peer
}

impl MoneyWeb {
    /// The page on `server`, its replica empty until the server fills it,
    /// and the sign-in machine told where this load starts: signed in
    /// already (the login this browser remembers there), back from signing
    /// in with a code to trade, or on its way to sign in.
    pub fn on_server(server: String, today: String) -> MoneyWeb {
        let mut app = MoneyWeb::over(replica(&server), today);
        let logins = Logins::new("money");
        let login = logins.recall(&server).filter(|l| l.expires_ms > ark_peer::now_ms());
        app.live = Some(Live {
            server: server.clone(),
            onboarding: Onboarding::new(self::app()),
            logins,
            login: None,
            timer: None,
        });
        #[cfg(target_arch = "wasm32")]
        app.peer.set_waker(Rc::new(|| arkui_web::send(Msg::Wake)));
        #[cfg(target_arch = "wasm32")]
        let (code, returned) = (ark_auth::web::take_code(), ark_auth::web::came_back());
        #[cfg(not(target_arch = "wasm32"))]
        let (code, returned) = (None, false);
        let remembered = Remembered {
            server: Some(server),
            login: login.map(Into::into),
            demo: false,
        };
        app.onboard(Event::Launched { remembered, code, returned });
        app
    }

    /// An event for the sign-in machine, and its effects carried out.
    pub fn onboard(&mut self, event: Event) {
        let Some(live) = &self.live else { return };
        for effect in live.onboarding.next(event) {
            self.effect(effect);
        }
    }

    /// One of the machine's effects: the login remembered or forgotten,
    /// the page sent to sign in, the code traded, the session opened or
    /// closed.
    fn effect(&mut self, effect: Effect) {
        let Some(live) = &mut self.live else { return };
        match effect {
            Effect::Remember { remembered } => match remembered.login {
                // The token is the cookie's: none is written here.
                Some(l) => live.logins.remember(
                    &live.server,
                    &ark_auth::Login {
                        token: String::new(),
                        ..l.into()
                    },
                ),
                None => live.logins.forget(&live.server),
            },
            Effect::Browse { server, .. } => {
                #[cfg(target_arch = "wasm32")]
                ark_auth::web::go_sign_in(&server, None);
                #[cfg(not(target_arch = "wasm32"))]
                let _ = server;
            }
            Effect::Exchange { server, code } => {
                #[cfg(target_arch = "wasm32")]
                wasm_bindgen_futures::spawn_local(async move {
                    let said = match ark_auth::web::session(&server, &code).await {
                        Ok(login) => Event::SignedIn { login: login.into() },
                        Err(why) => Event::SignInFailed { why },
                    };
                    arkui_web::send(Msg::Onboard(said));
                });
                #[cfg(not(target_arch = "wasm32"))]
                let _ = (server, code);
            }
            // A join link is a phone's: the page is signed in by its own
            // server's sign-in, and never sends `Joined`.
            Effect::Redeem { .. } => self.onboard(Event::SignInFailed {
                why: "a join link is opened in the app".into(),
            }),
            Effect::OpenSession { login, .. } => {
                let login: ark_auth::Login = login.into();
                if live.login.as_ref() != Some(&login) {
                    self.peer.login(ark_peer::who(&login), ark_auth::COOKIE_HELLO.into());
                    live.login = Some(login);
                }
                self.wake();
            }
            Effect::CloseSession => {
                // The session ended over the socket while it is up (its own
                // to end: `session.revoke`), then the cookie forgotten.
                if live.login.take().is_some() && self.peer.linked {
                    let _ = self.peer.sign_out();
                }
                #[cfg(target_arch = "wasm32")]
                {
                    let server = live.server.clone();
                    wasm_bindgen_futures::spawn_local(async move {
                        let _ = ark_auth::web::forget_session(&server).await;
                    });
                }
            }
            Effect::FindServer { .. } | Effect::OpenDemo => {}
        }
    }

    /// What the socket said, taken in: the views brought up to it, a
    /// session the server turned away handed to the machine, the session
    /// that was opening shown once the server's budget is here (or the
    /// server cannot be reached, and nothing more is coming soon), and the
    /// next pump set for the peer's next deadline.
    pub fn wake(&mut self) {
        let p = self.peer.pump();
        // A refusal undoes its change in the views, as on every device.
        let _ = self.peer.take_rejections();
        self.store.settle(&mut self.peer);
        if let Some(reason) = p.denied {
            self.onboard(Event::Denied { reason });
        }
        let opening = self.live.as_ref().is_some_and(|l| matches!(l.onboarding.step(), Step::Opening { .. }));
        let filling = self.peer.machine(ark::schema::Space::Durable).is_some_and(|m| m.fill.is_some());
        let here = self.peer.linked && !filling && self.peer.backlog() == 0 && self.peer.cursor() > 0;
        if opening && (here || !p.closed.is_empty()) {
            self.onboard(Event::Synced);
        }
        self.schedule();
    }

    /// A pump at the peer's next deadline, the one before it cleared.
    fn schedule(&mut self) {
        let Some(live) = &mut self.live else { return };
        #[cfg(target_arch = "wasm32")]
        {
            use wasm_bindgen::JsCast;
            let Some(window) = web_sys::window() else { return };
            if let Some(t) = live.timer.take() {
                window.clear_timeout_with_handle(t);
            }
            let Some(due) = self.peer.deadline() else { return };
            let wait = due.saturating_sub(ark_peer::monotonic_ms()).min(i32::MAX as u64) as i32;
            let ring = wasm_bindgen::closure::Closure::once_into_js(|| arkui_web::send(Msg::Wake));
            live.timer = window
                .set_timeout_with_callback_and_timeout_and_arguments_0(ring.unchecked_ref(), wait)
                .ok();
        }
        let _ = &live.timer;
    }

    /// The sign-in page, while the app is not shown: what the machine says,
    /// and Sign In where a sign-in waits for the person.
    pub fn welcome(&self, live: &Live) -> arkui_web::AnyView {
        let said = live.onboarding.said();
        let actions = match live.onboarding.step() {
            Step::SignIn { phase: Phase::Idle, .. } => vec![("Sign In", true, Msg::Onboard(Event::OpenSignIn))],
            _ => vec![],
        };
        arkui_web::welcome(arkui_web::Welcome {
            title: self::app().name,
            status: said.status,
            error: said.error,
            actions,
        })
    }
}
