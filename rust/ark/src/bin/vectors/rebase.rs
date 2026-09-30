//! `rebase/` (§10, §11, §15): three peers and an authority through one
//! scripted session, asserted step by step; and a seeded fleet, whose
//! script and final hash are the vector.

use std::collections::BTreeMap;

use ark::eval::{Args, Ctx};
use ark::hash::{closures, state_hash, Closure, FnHash};
use ark::ir::{module_value, Expr, Op, Stmt};
use ark::log::{Entry, Page, Seq};
use ark::peer::{local_commit, AdoptError, Authority, Changes, Replica, Sequenced};
use ark::protocol::{change_value, entry_value};
use ark::sim::Sim;
use ark::store::{Change, MemoryStore, Store};
use ark::value::{hex, Id, Value};

use super::demo::{self, hash_of, id_n};
use super::json::{json, obj, quoted};
use super::{claim, Out};

fn args<const N: usize>(pairs: [(&str, Value); N]) -> Args {
    pairs.into_iter().map(|(k, v)| (k.to_string(), v)).collect()
}

/// The whole of §11 on one scenario, asserted as it goes: the file is
/// written only if every claim holds.
pub fn three_peers(out: &Out) {
    out.dir("rebase/");
    let m = demo::module();
    let sch = m.schema.clone();
    let bodies = closures(&m);
    let h_create = hash_of(&m, "create_playlist");
    let h_add = hash_of(&m, "add_to_playlist");
    let pid = id_n(1);
    let ctx = |who: &str| Ctx::new(who, format!("{who}-session"));
    let now = Args::new();
    let add_args = |k: u8| args([("playlist_id", Value::Id(pid)), ("track_id", Value::text(format!("t{k}")))]);
    let must = |what: &str, r: Result<Entry, ark::store::Refusal>| r.unwrap_or_else(|e| panic!("{what}: {e:?}"));
    let open = |bodies: BTreeMap<FnHash, Closure>| Replica::open(sch.clone(), bodies, MemoryStore::empty(sch.clone()), 0, vec![]);
    let fresh = || open(bodies.clone());
    // The authority answers one pushed entry.
    let push = |a: &mut Authority, e: &Entry| match a.sequence_entry(e) {
        Sequenced::Appended(n, facts) => (n, facts),
        other => panic!("push: {other:?}"),
    };
    let pos_of = |r: &Replica, k: u8| {
        r.view
            .get("item", &[Value::Id(pid), Value::text(format!("t{k}"))])
            .and_then(|row| row.get("pos").cloned())
    };

    let mut auth = Authority::new(sch.clone(), bodies.clone());
    let (mut alice, mut bob) = (fresh(), fresh());
    // step 1: alice creates the playlist; everybody sees it
    let e1 = must(
        "create",
        alice.mutate(
            id_n(101),
            &ctx("alice"),
            &h_create,
            &args([("id", Value::Id(pid))]),
            &args([("name", Value::text(" Favorites "))]),
        ),
    );
    let (s1, _) = push(&mut auth, &e1);
    alice.ack(&e1.id, s1);
    alice.settle();
    bob.receive(s1, e1.clone());
    bob.settle();
    claim("the playlist was sequenced first", s1 == 1);
    claim(
        "alice's name was trimmed",
        alice.view.get("playlist", &[Value::Id(pid)]).and_then(|r| r.get("name").cloned()) == Some(Value::text("Favorites")),
    );
    claim("alice and bob agree after step 1", alice.verify_at() == bob.verify_at());
    // step 2: alice goes dark. bob adds two tracks; alice adds one alone.
    let e2 = must("bob adds 1", bob.mutate(id_n(102), &ctx("bob"), &h_add, &now, &add_args(1)));
    let (s2, _) = push(&mut auth, &e2);
    bob.ack(&e2.id, s2);
    bob.settle();
    let e3 = must("bob adds 2", bob.mutate(id_n(103), &ctx("bob"), &h_add, &now, &add_args(2)));
    let (s3, _) = push(&mut auth, &e3);
    bob.ack(&e3.id, s3);
    bob.settle();
    let _ = alice.take_changes(); // the ack rebuilt her view; a screen has drawn it since
    let e9 = must("alice adds 9 alone", alice.mutate(id_n(109), &ctx("alice"), &h_add, &now, &add_args(9)));
    claim("alone, alice's track is first on her view", pos_of(&alice, 9) == Some(Value::Int(1)));
    claim(
        "a local mutation reports its changes, not a rebuild",
        matches!(alice.take_changes(), Changes::Applied(ref c) if c.len() == 1),
    );
    claim(
        "bob's tracks are 1 and 2 on his view",
        pos_of(&bob, 1) == Some(Value::Int(1)) && pos_of(&bob, 2) == Some(Value::Int(2)),
    );
    // step 3: alice comes back. bob's entries land; her pending replays on top.
    // Two pumps, one landing each (plan-perf R8): two rebases.
    alice.receive(s2, e2.clone());
    alice.settle();
    alice.receive(s3, e3.clone());
    alice.settle();
    // Each landing undoes her track, lands bob's, and puts hers back on
    // top: three transitions a view can be told, twice (plan-perf R2).
    claim(
        "the rebase is reported as its transitions",
        matches!(alice.take_changes(), Changes::Applied(ref c) if c.len() == 6),
    );
    claim("after the rebase alice's track is third", pos_of(&alice, 9) == Some(Value::Int(3)));
    claim("alice's confirmed state is bob's", alice.verify_at() == bob.verify_at());
    // alice pushes what she did alone; it lands after everything that
    // happened while she was away
    let (s9, f9) = push(&mut auth, &e9);
    alice.receive_facts(s9, f9.clone());
    alice.ack(&e9.id, s9);
    alice.settle();
    bob.receive(s9, e9.clone());
    bob.settle();
    claim("nothing is pending on alice once acked", alice.pending.is_empty());
    claim(
        "with nothing pending the ack costs no rebuild",
        matches!(alice.take_changes(), Changes::Applied(_)),
    );
    claim("alice's view is her confirmed store", alice.view == alice.confirmed);
    claim(
        "the authority's facts say pos 3 too",
        f9.iter()
            .any(|c| matches!(c, Change::Add(_, row) if row.get("pos") == Some(&Value::Int(3)))),
    );
    claim(
        "three replicas, one hash",
        alice.verify_at() == bob.verify_at() && bob.verify_at().1 == state_hash(&auth.store),
    );
    // a duplicate delivery changes nothing
    let before = bob.clone();
    bob.receive(s2, e2.clone());
    bob.settle();
    claim("a duplicate delivery is a no-op", bob == before);
    // carol holds no generated code at all: she applies by facts
    let sequenced = [(s1, &e1), (s2, &e2), (s3, &e3), (s9, &e9)];
    let mut carol = open(BTreeMap::new());
    for (n, e) in sequenced {
        carol.receive(n, e.clone());
    }
    claim("without closures carol asks for every entry's facts", carol.needs() == vec![1, 2, 3, 4]);
    let facts_of = |n: Seq| auth.log.entries.get(&n).map(|(_, f)| f.clone()).expect("no facts");
    for n in 1..=4 {
        carol.receive_facts(n, facts_of(n));
    }
    carol.settle();
    claim(
        "by facts alone carol reaches the same state",
        carol.verify_at() == bob.verify_at() && carol.diverged.is_empty(),
    );
    // dave's build of add_to_playlist is wrong: it steps by two. Facts catch it.
    let mut wrong = bodies.clone();
    for st in wrong.get_mut(&h_add).expect("add_to_playlist").function.body.iter_mut() {
        if let Stmt::Insert(_, Expr::Struct(fs), _) = st {
            if let Some(Expr::Op(Op::Add, xs)) = fs.get_mut("pos") {
                xs[1] = Expr::Lit(Value::Int(2));
            }
        }
    }
    let mut dave = open(wrong);
    for (n, e) in sequenced {
        dave.receive_with(n, e.clone(), facts_of(n));
    }
    dave.settle();
    claim("a divergent runtime is detected", dave.diverged == vec![2, 3, 4]);
    claim("and healed by the facts", dave.verify_at() == bob.verify_at());
    // eve has no server: she is her own authority, and later hands the log over
    let mut eve = fresh();
    let mut eve_auth = Authority::new(sch.clone(), bodies.clone());
    must(
        "eve creates",
        eve.mutate(
            id_n(201),
            &ctx("eve"),
            &h_create,
            &args([("id", Value::Id(id_n(2)))]),
            &args([("name", Value::text("Road"))]),
        ),
    );
    must(
        "eve adds",
        eve.mutate(
            id_n(202),
            &ctx("eve"),
            &h_add,
            &now,
            &args([("playlist_id", Value::Id(id_n(2))), ("track_id", Value::text("t5"))]),
        ),
    );
    local_commit(&mut eve_auth, &mut eve);
    claim(
        "alone, eve confirms her own intents",
        eve.cursor == 2 && eve.pending.is_empty() && eve.view == eve.confirmed,
    );
    claim("and her state is her authority's", eve.verify_at().1 == state_hash(&eve_auth.store));
    let adopted = Authority::adopt(sch.clone(), bodies.clone(), &eve_auth.log);
    claim(
        "a server adopts her log by replaying it",
        adopted.is_ok_and(|a| state_hash(&a.store) == eve.verify_at().1),
    );
    let mut tampered = eve_auth.log.clone();
    for c in &mut tampered.entries.get_mut(&2).expect("entry 2").1 {
        if let Change::Add(_, row) = c {
            row.insert("pos".into(), Value::Int(99));
        }
    }
    claim(
        "a log whose facts were touched is refused",
        Authority::adopt(sch.clone(), bodies.clone(), &tampered).err() == Some(AdoptError::FactsDiffer(2)),
    );
    // compaction: the authority moves its horizon to 2
    let mut compacted = auth.clone();
    claim("the horizon moves to 2", compacted.compact(2));
    claim(
        "a peer at 0 is sent the snapshot",
        matches!(compacted.page(0, 10), Page::BelowHorizon(ref sn) if sn.seq == 2),
    );
    claim(
        "a peer at 2 is sent the tail",
        matches!(compacted.page(2, 10), Page::Entries(ref es, false) if es.iter().map(|(n, _, _)| *n).collect::<Vec<_>>() == vec![3, 4]),
    );
    claim(
        "the state at the head, from facts, is the head state",
        compacted.log.state_at(4).map(|st| state_hash(&st)) == Some(state_hash(&compacted.store)),
    );
    let entries: Vec<Value> = auth
        .log
        .entries
        .iter()
        .map(|(n, (e, _))| match entry_value(e) {
            Value::Struct(mut fields) => {
                fields.insert("seq".into(), Value::Int(*n));
                Value::Struct(fields)
            }
            other => other,
        })
        .collect();
    let facts: Vec<Value> = auth
        .log
        .entries
        .values()
        .map(|(_, f)| Value::List(f.iter().map(change_value).collect()))
        .collect();
    out.write(
        "rebase/three-peers.json",
        &obj(&[
            ("module", json(&module_value(&m))),
            ("entries", json(&Value::List(entries))),
            ("facts", json(&Value::List(facts))),
            ("alice_alone_pos_of_9", json(&Value::Int(1))),
            ("alice_after_rebase_pos_of_9", json(&Value::Int(3))),
            ("final_hash", quoted(&hex(&state_hash(&auth.store)))),
            ("final_store", json(&auth.store.store_value())),
        ]),
    );
    println!("  final hash    {}", hex(&state_hash(&auth.store)));
}

/// One step of a fleet's script.
enum Move {
    /// Peer, track: peer adds `"{peer}-{track}"` to the playlist.
    Add(i64, i64),
    Partition(i64),
    Heal(i64),
    Step,
}

impl Move {
    fn value(&self) -> Value {
        let t = |s: &str| ("t", Value::text(s));
        match self {
            Move::Add(i, k) => Value::record(vec![t("add"), ("peer", Value::Int(*i)), ("track", Value::text(format!("{i}-{k}")))]),
            Move::Partition(i) => Value::record(vec![t("partition"), ("peer", Value::Int(*i))]),
            Move::Heal(i) => Value::record(vec![t("heal"), ("peer", Value::Int(*i))]),
            Move::Step => Value::record(vec![t("step")]),
        }
    }
}

/// An id from a counter: its two high bytes, then zeros. The fleet's ids
/// are numbered this way so that thousands of them stay distinct.
fn id_of(k: i64) -> Id {
    let mut id = [0u8; 16];
    id[0] = (k / 256) as u8;
    id[1] = (k % 256) as u8;
    id
}

/// A seeded fleet of three over the demo domain: adds, partitions, heals
/// and two hundred random deliveries with duplicates and drops; then
/// settle, and every replica must hash as the authority does.
pub fn fleet(out: &Out) {
    out.dir("rebase/ (fleet)");
    let m = demo::module();
    let h_create = hash_of(&m, "create_playlist");
    let h_add = hash_of(&m, "add_to_playlist");
    let pid = id_of(1);
    let (clients, seed): (i64, u64) = (3, 7);
    let mut sim = Sim::new(m.schema.clone(), closures(&m), clients, seed);
    // one playlist, created by peer 0 and delivered to all
    sim.mutate(
        0,
        id_of(1000),
        &h_create,
        &args([("id", Value::Id(pid))]),
        &args([("name", Value::text("Fleet"))]),
    );
    sim.settle();
    // a scripted mess: adds from everyone, a partition, more adds, random deliveries
    let mut script: Vec<Move> = Vec::new();
    for k in 1..=4 {
        script.extend((0..=2).map(|i| Move::Add(i, k)));
    }
    script.push(Move::Partition(2));
    script.extend((5..=7).map(|k| Move::Add(2, k)));
    script.extend([Move::Add(0, 8), Move::Add(1, 9)]);
    script.extend((0..60).map(|_| Move::Step));
    script.push(Move::Partition(0));
    script.extend([Move::Add(1, 10), Move::Add(0, 11)]);
    script.extend((0..60).map(|_| Move::Step));
    script.extend([Move::Heal(0), Move::Heal(2)]);
    script.extend((0..80).map(|_| Move::Step));
    let mut n = 0;
    for op in &script {
        match op {
            Move::Add(i, k) => {
                let a = args([("playlist_id", Value::Id(pid)), ("track_id", Value::text(format!("{i}-{k}")))]);
                sim.mutate(*i, id_of(2000 + n), &h_add, &Args::new(), &a);
                n += 1;
            }
            Move::Partition(i) => sim.partition(*i),
            Move::Heal(i) => sim.heal(*i),
            Move::Step => sim.step(),
        }
    }
    sim.settle();
    let (head, server_hash) = sim.server_hash();
    let hashes = sim.client_hashes();
    assert!(
        hashes.iter().all(|(_, n, h)| *n == head && *h == server_hash),
        "fleet did not converge: {:?} vs {}",
        hashes.iter().map(|(i, n, h)| (i, n, hex(h))).collect::<Vec<_>>(),
        hex(&server_hash)
    );
    claim("fleet: something still pending after settle", sim.quiet());
    let rejected: Vec<_> = sim
        .clients
        .iter()
        .flat_map(|(i, c)| c.replica.rejections.iter().map(move |r| (i, r)))
        .collect();
    assert!(rejected.is_empty(), "fleet: rejections: {rejected:?}");
    assert!(head >= 12, "fleet: too few entries landed: {head}");
    let store = &sim.server.authority.store;
    println!(
        "  {} items on the playlist after {head} entries; hash {}",
        store.rows("item").len(),
        hex(&server_hash)
    );
    let parts = |head: Seq, expect: bool| {
        let mut parts = vec![
            ("module", json(&module_value(&m))),
            ("clients", json(&Value::Int(clients))),
            ("seed", json(&Value::Int(seed as i64))),
            ("script", json(&Value::List(script.iter().map(Move::value).collect()))),
            ("expected_head", json(&Value::Int(head))),
            ("expected_hash", quoted(&hex(&server_hash))),
            ("final_store", json(&store.store_value())),
        ];
        if expect {
            parts.push(("expect", quoted("fail")));
        }
        obj(&parts)
    };
    // One entry more than landed, under the hash that did.
    out.write(&format!("rebase/falsify/fleet-seed-{seed}-one-more.json"), &parts(head + 1, true));
    out.write(&format!("rebase/fleet-seed-{seed}.json"), &parts(head, false));
}
