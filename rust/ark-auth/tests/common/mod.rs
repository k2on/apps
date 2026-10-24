//! A tiny app taking `ark-auth`'s two tables in, and a hub over its store
//! in memory: what the tests sign in to.
#![allow(dead_code)]

use std::sync::{Arc, Mutex};

use ark::prelude::*;
use ark::store::{Change, Store};
use ark_auth::server::Hub;
use ark_auth::SignIn;

/// The app's own table.
#[derive(Clone, Debug, PartialEq, Eq, Default, Serialize, Deserialize, Row)]
pub struct Note {
    #[ark(key)]
    pub id: String,
}

#[derive(Schema)]
pub struct App {
    pub note: Note,
    pub sign_in: ark_auth::SignIn,
    pub role: ark_auth::Role,
}

ark_auth::install!(App);

fn all<R>(_: &Ctx, _: &R) -> bool {
    true
}

pub fn domain() -> Domain {
    Domain::new::<App>("app", "1")
        .with(ark_auth::register::<App>)
        .scope("feed", ark_auth::feed(Scope::<App>::new().on::<Note>(all)))
}

/// The hub: a store every sign-in is written to, and what its authority was
/// asked to author.
#[derive(Clone)]
pub struct Memory(pub Arc<Mutex<Store>>, pub Authored);

/// Each function the authority was asked to author, with its input.
pub type Authored = Arc<Mutex<Vec<(String, Vec<u8>)>>>;

impl Default for Memory {
    fn default() -> Memory {
        Memory::new()
    }
}

impl Memory {
    pub fn new() -> Memory {
        Memory(
            Arc::new(Mutex::new(Store::new(domain().schema.clone(), ark::schema::Space::Durable))),
            Arc::default(),
        )
    }

    pub fn sign_ins(&self) -> Vec<SignIn> {
        let s = self.0.lock().unwrap();
        ark::db::read::<App, _>(&s, |db| Ok::<_, Refusal>(db.sign_in.scan())).unwrap()
    }
}

impl Hub for Memory {
    fn read(&self, f: Box<dyn FnOnce(&Store) + Send>) {
        f(&self.0.lock().unwrap())
    }

    fn write(&self, c: Change) -> Result<(), String> {
        self.0.lock().unwrap().apply(&c).map_err(|r| r.0)
    }

    fn author(&self, func: &str, input: Vec<u8>) -> Result<(), String> {
        self.1.lock().unwrap().push((func.into(), input));
        Ok(())
    }
}

/// A context as `user` on `session` holding `roles`, at `now`.
pub fn on(user: &str, session: &str, roles: &[&str], now: i64) -> Ctx {
    Ctx {
        id: ark::log::EntryId::of(session, 1),
        actor: user.into(),
        session: session.into(),
        roles: roles.iter().map(|r| r.to_string()).collect(),
        now,
        ..Default::default()
    }
}
