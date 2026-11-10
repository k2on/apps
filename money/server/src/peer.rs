//! `money-peer`: a device of money's with no window — signed in to a dev
//! server by name, its replica in memory, synced over the socket as a
//! phone's or the desktop's is (`ark_peer::Peer`, the views a screen holds
//! in `ark_peer::store::Store`). What a test or a script stands another
//! device up with: the browser test (`nix/web-sync.nix`) has one wait for
//! what the page authored and author what the page must then see.
//!
//! ```text
//! money-peer SERVER USER wait PAYEE [SECS]    a transaction from PAYEE is in the register
//! money-peer SERVER USER add PAYEE AMOUNT     one from PAYEE on the first open budget
//!                                             account, AMOUNT dollars out, taken by the server
//! ```
//!
//! Exits 0 once it holds, 1 if it does not within SECS (30).

use std::time::{Duration, Instant};

use anyhow::{anyhow, bail, Result};
use ark_peer::store::Store;
use ark_peer::{Config, Memory, Peer};
use money_core::{AccountLine, AddTransaction, MoneyChange, MoneyView, TxnLine};

const USAGE: &str = "usage: money-peer SERVER USER wait PAYEE [SECS] | money-peer SERVER USER add PAYEE AMOUNT";

fn main() -> Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let [server, user, verb, payee, rest @ ..] = &args[..] else {
        bail!("{USAGE}");
    };
    let login = ark_auth::client::login(server, Some(user), |url| {
        eprintln!("money-peer: a dev server signs in by name; not {url}")
    })
    .map_err(|e| anyhow!("signing in as {user}: {e}"))?;
    let mut peer = Peer::open_as(
        Config::of(money_core::shared()),
        Box::new(Memory::default()),
        Some((ark_peer::who(&login), login.token.clone())),
    );
    let (bell, rang) = ark_peer::link::bell();
    peer.set_waker(bell);
    peer.connect(&ark_auth::socket_url(server));
    let mut store = Store::new();
    for v in [MoneyView::Accounts, MoneyView::Register { account: None }] {
        store.hold(&mut peer, v);
    }
    let secs = match (verb.as_str(), rest) {
        ("wait", [s]) => s.parse().map_err(|_| anyhow!("SECS is a number: {s}"))?,
        _ => 30,
    };
    let until = Instant::now() + Duration::from_secs(secs);
    // Pumped until `done` holds over what the views show, or the time is up.
    let run = |peer: &mut Peer, store: &mut Store<MoneyView>, done: &mut dyn FnMut(&mut Peer, &Store<MoneyView>) -> bool| -> Result<()> {
        loop {
            let p = peer.pump();
            store.settle(peer);
            if let Some(why) = p.denied {
                bail!("the server turned {user} away: {why}");
            }
            if let Some((_, why)) = peer.take_rejections().into_iter().next() {
                bail!("the server refused: {why}");
            }
            if peer.linked && done(peer, store) {
                return Ok(());
            }
            if Instant::now() >= until {
                bail!("not within {secs}s");
            }
            ark_peer::link::wait(peer, &rang, Duration::from_millis(50));
        }
    };
    let has = |store: &Store<MoneyView>, payee: &str| {
        store
            .rows::<TxnLine>(&MoneyView::Register { account: None })
            .iter()
            .any(|t| t.payee == payee)
    };
    match (verb.as_str(), rest) {
        ("wait", [] | [_]) => {
            run(&mut peer, &mut store, &mut |_, s| has(s, payee))?;
            eprintln!("money-peer: {user} sees {payee}");
        }
        ("add", [amount]) => {
            let cents = ark_format::parse_amount(amount, &ark_format::NumberStyle::us()).ok_or_else(|| anyhow!("AMOUNT is dollars: {amount}"))?;
            // Caught up first: the accounts are the server's.
            run(&mut peer, &mut store, &mut |p, s| {
                p.cursor() > 0 && !s.rows::<AccountLine>(&MoneyView::Accounts).is_empty()
            })?;
            let account = store
                .rows::<AccountLine>(&MoneyView::Accounts)
                .iter()
                .find(|a| a.on_budget && !a.closed)
                .map(|a| a.id)
                .ok_or_else(|| anyhow!("no open budget account"))?;
            let change = MoneyChange::AddTransaction(AddTransaction {
                account_id: account,
                date: money_core::dates::day_of_ms(ark_peer::now_ms()),
                payee: payee.clone(),
                category_id: None,
                memo: String::new(),
                amount: -cents.abs(),
                cleared: false,
                transfer_to: None,
            });
            peer.mutate_bytes(change.name(), change.input()).map_err(|e| anyhow!("{e}"))?;
            run(&mut peer, &mut store, &mut |p, s| p.pending_len() == 0 && has(s, payee))?;
            eprintln!("money-peer: {user} added {payee}");
        }
        _ => bail!("{USAGE}"),
    }
    Ok(())
}
