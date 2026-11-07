//! A server's work as a device of its own (the `server` feature): a
//! **worker**, in the server's process ([`Handle::worker`]).
//!
//! What a server does to the world outside — reads a folder, downloads a
//! feed, asks a bank — it does as somebody: a worker is a device signed in
//! as an account of the server's own (`ark_auth::server::Auth::own`, its
//! roles the account's and those the server grants it), dialling its
//! server in this process with that login's token, judged as every
//! connection is and fed what its scope says. It is a [`Peer`] on a thread
//! of its own ([`Handle::spawn`]), so its handlers ([`Peer::handle`]) run
//! each on a thread of its own, and what it writes is an intent like any
//! device's, confirmed or refused by the server, every refusal said
//! (`ark`'s `refused`). The server's files are its files: a job naming
//! one runs once the server has it.

use std::sync::Arc;

use ark_auth::server::Auth;
use ark_auth::Account;

use crate::{files, Authority, Config, Disk, Handle, Peer};

impl Handle<Authority> {
    /// A worker of this server's, signed in by `auth` as `account` on
    /// `device`, its replica on `disk`, linked and on a thread of its own;
    /// or why it cannot sign in. Each start is a login of its own: what
    /// the replica kept pending is authored again under it.
    pub fn worker(&self, auth: &Arc<Auth>, account: &Account, device: &str, disk: Box<dyn Disk>) -> Result<Handle<Peer>, String> {
        let login = auth.own(account, device)?;
        let who = crate::who(&login);
        let (log, kept) = self
            .with(|h| {
                let kept = match &h.core.files {
                    files::Store::Dir(d) => Some(d.clone()),
                    files::Store::Memory(_) => None,
                };
                (h.config.log.clone(), kept)
            })
            .ok_or("the server stopped")?;
        let mut peer = Peer::open(Config::of(log), who.clone(), disk);
        if let Some(d) = kept {
            peer.keep_files(files::Store::Dir(d));
        }
        peer.sign_in(who);
        peer.set_token(Some(login.token));
        peer.connect_with("", self.dialer(auth.authenticate()));
        Ok(Handle::spawn(peer))
    }
}
