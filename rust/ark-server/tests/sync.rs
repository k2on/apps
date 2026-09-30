//! Two ark-clients syncing through an in-process ark-server over real
//! sockets, against the demo module (spec/AUTHORING.md Appendix B).

mod common;

use ark::value::Value;
use ark_client::{args, demo, Options, Peer, Update};
use common::*;

#[test]
fn two_peers_sync_and_an_offline_edit_rebases_on_top() {
    let rt = runtime();
    let running = serve(&rt, None, |b| b);
    let mut alice = peer(&running, "alice");
    let mut bob = peer(&running, "bob");

    alice.mutate("create_playlist", args([("name", Value::text("  Road trip "))])).unwrap();
    pump_until(&mut [&mut alice, &mut bob], |ps| ps.iter().all(|p| p.cursor() == 1));
    let list = playlist(&bob, "Road trip");
    alice
        .mutate(
            "add_to_playlist",
            args([("playlist_id", Value::Id(list)), ("track_id", Value::text("a"))]),
        )
        .unwrap();
    pump_until(&mut [&mut alice, &mut bob], |ps| ps.iter().all(|p| p.cursor() == 2));

    // Bob holds the playlist as a maintained view, and goes offline.
    let mut view = bob.view("items", args([("playlist_id", Value::Id(list))])).unwrap();
    assert!(view.incremental(), "a single select is maintained, not re-run");
    assert_eq!(rows_tracks(view.rows()), ["a"]);
    let _ = bob.take_changes();
    bob.disconnect();
    assert!(!bob.linked());

    // Both add at the end: bob's lands at 2 locally, alice's at 2 on the log.
    alice
        .mutate(
            "add_to_playlist",
            args([("playlist_id", Value::Id(list)), ("track_id", Value::text("b"))]),
        )
        .unwrap();
    bob.mutate(
        "add_to_playlist",
        args([("playlist_id", Value::Id(list)), ("track_id", Value::text("c"))]),
    )
    .unwrap();
    let changes = bob.take_changes();
    let c = bob.query("items", &args([("playlist_id", Value::Id(list))])).unwrap().as_list()[1].clone();
    assert_eq!(
        view.update(&bob, &changes).unwrap(),
        Update::Patched(vec![ark_client::Patch::Insert { at: 1, node: c }])
    );
    assert_eq!(rows_tracks(view.rows()), ["a", "c"], "optimistic");
    pump_until(&mut [&mut alice], |ps| ps[0].pending_len() == 0);
    assert_eq!(tracks(&alice, list), ["a", "b"]);

    // Back online: the confirmed entry lands under bob's pending one, which
    // replays on top — the rebase, visible.
    bob.reconnect();
    pump_until(&mut [&mut alice, &mut bob], |ps| {
        ps[1].pending_len() == 0 && ps.iter().all(|p| p.cursor() == 4)
    });
    assert_eq!(tracks(&bob, list), ["a", "b", "c"]);
    assert_eq!(tracks(&alice, list), ["a", "b", "c"]);
    let changes = bob.take_changes();
    assert!(
        matches!(changes, ark_client::Changes::Applied(_)),
        "a confirmed entry under a pending one is the transitions the rebase made: {changes:?}"
    );
    let update = view.update(&bob, &changes).unwrap();
    assert!(matches!(update, Update::Patched(_)), "{update:?}");
    assert_eq!(rows_tracks(view.rows()), ["a", "b", "c"]);
    assert!(bob.take_rejections().is_empty());

    // …and all three agree with the authority.
    alice.verify();
    bob.verify();
    pump_until(&mut [&mut alice, &mut bob], |ps| ps.iter().all(|p| !p.agreed().is_empty()));
    assert_eq!(alice.agreed(), [(4, true)]);
    assert_eq!(bob.agreed(), [(4, true)]);
    assert_eq!(bob.replica().verify_at(), alice.replica().verify_at());
    rt.block_on(running.stop());
}

#[test]
fn a_maintained_view_follows_the_log_patch_by_patch() {
    let rt = runtime();
    let running = serve(&rt, None, |b| b);
    let mut alice = peer(&running, "alice");
    let mut bob = peer(&running, "bob");
    alice.mutate("create_playlist", args([("name", Value::text("Focus"))])).unwrap();
    pump_until(&mut [&mut alice, &mut bob], |ps| ps.iter().all(|p| p.cursor() == 1));
    let list = playlist(&bob, "Focus");
    let mut view = bob.view("items", args([("playlist_id", Value::Id(list))])).unwrap();
    let _ = bob.take_changes();
    let mut spliced: Vec<Value> = view.rows().to_vec();
    for (i, t) in ["x", "y", "z"].iter().enumerate() {
        alice
            .mutate("add_to_playlist", args([("playlist_id", Value::Id(list)), ("track_id", Value::text(*t))]))
            .unwrap();
        pump_until(&mut [&mut alice, &mut bob], |ps| ps[1].cursor() == 2 + i as i64);
        let changes = bob.take_changes();
        match view.update(&bob, &changes).unwrap() {
            Update::Patched(ps) => ark_client::splice(&mut spliced, &ps),
            other => panic!("nothing of bob's was pending, so a patch, not {other:?}"),
        }
    }
    assert_eq!(rows_tracks(&spliced), ["x", "y", "z"]);
    assert_eq!(spliced, view.rows());
    assert_eq!(rows_tracks(view.rows()), tracks(&bob, list));
    rt.block_on(running.stop());
}

#[test]
fn pending_intents_survive_a_restart_and_so_does_the_servers_log() {
    let rt = runtime();
    let server_data = tempfile::tempdir().unwrap();
    let client_data = tempfile::tempdir().unwrap();
    let open = || Peer::open_path(demo::domain(), client_data.path(), Options::dev("alice").with_timing(quick())).unwrap();

    // Authored with no server in existence, and the program closed.
    {
        let mut p = open();
        p.mutate("create_playlist", args([("name", Value::text("Offline"))])).unwrap();
        assert_eq!(p.pending_len(), 1);
    }
    let mut p = open();
    assert_eq!(p.pending_len(), 1, "the intent was durable");
    assert!(ark::store::Store::scan(p.store(), "playlist").len() == 1, "and is in the view on reopen");

    let running = serve(&rt, Some(server_data.path()), |b| b);
    p.connect(&running.sync_url());
    pump_until(&mut [&mut p], |ps| ps[0].pending_len() == 0);
    assert_eq!(p.cursor(), 1);
    let claim = p.replica().verify_at();
    drop(p);
    rt.block_on(running.stop());

    // The server restarts on its data, the client on its own.
    let running = serve(&rt, Some(server_data.path()), |b| b);
    assert!(server_data.path().join("log.ark-log").is_file());
    let mut p = open();
    assert_eq!(p.cursor(), 1, "the confirmed store was durable");
    let mut carol = peer(&running, "carol");
    p.connect(&running.sync_url());
    pump_until(&mut [&mut p, &mut carol], |ps| ps[1].cursor() == 1 && ps[0].linked());
    assert_eq!(carol.replica().verify_at(), claim);
    // Opened the other way, a directory leaves the server rather than
    // refusing (`docs/plan-alone.md` §1): its fork is where it stood, and
    // it goes on from there alone.
    drop(p);
    let alone = Peer::open_path(demo::domain(), client_data.path(), Options::alone("alice")).unwrap();
    assert_eq!(
        (alone.cursor(), alone.status().fork.cursor, alone.status().link.as_str()),
        (1, 1, "alone")
    );
    rt.block_on(running.stop());
}

#[test]
fn a_peer_alone_is_its_own_authority_and_keeps_nothing_pending() {
    let data = tempfile::tempdir().unwrap();
    {
        let mut p = Peer::open_path(demo::domain(), data.path(), Options::alone("me")).unwrap();
        p.mutate("create_playlist", args([("name", Value::text("Mine"))])).unwrap();
        assert_eq!(p.pending_len(), 0);
        assert_eq!(p.cursor(), 1);
        // `insert(..).on(user_id, name)`: a second of the same name is an
        // entry in the log that writes nothing.
        p.mutate("create_playlist", args([("name", Value::text("Mine"))])).unwrap();
        assert_eq!(ark::store::Store::scan(p.store(), "playlist").len(), 1);
        assert_eq!(p.cursor(), 2);
        let err = p.mutate("create_playlist", args([("name", Value::text("  "))])).unwrap_err();
        assert_eq!(err.to_string(), "a playlist needs a name");
        let checked = p.check("create_playlist", &args([("name", Value::text(" "))])).unwrap();
        assert_eq!(checked.messages, [("name".to_string(), "a playlist needs a name".to_string())]);
        p.verify();
        assert_eq!(p.agreed(), [(2, true)]);
        assert_eq!(p.status().link, "alone");
    }
    let p = Peer::open_path(demo::domain(), data.path(), Options::alone("me")).unwrap();
    assert_eq!(p.cursor(), 2);
}

#[test]
fn a_frame_that_is_not_the_protocol_closes_that_socket_and_nothing_else() {
    use futures_util::{SinkExt, StreamExt};
    let rt = runtime();
    let running = serve(&rt, None, |b| b);
    let mut good = peer(&running, "alice");
    pump_until(&mut [&mut good], |ps| ps[0].linked());
    let url = running.sync_url();
    let closed = rt.block_on(async move {
        let (mut bad, _) = tokio_tungstenite::connect_async(url).await.unwrap();
        bad.send(tokio_tungstenite::tungstenite::Message::Text("hello".into())).await.unwrap();
        tokio::time::timeout(std::time::Duration::from_secs(5), async {
            loop {
                match bad.next().await {
                    Some(Ok(tokio_tungstenite::tungstenite::Message::Close(_))) | None | Some(Err(_)) => break,
                    _ => {}
                }
            }
        })
        .await
    });
    assert!(closed.is_ok(), "the server closes a socket that does not speak the protocol");
    good.mutate("create_playlist", args([("name", Value::text("Still here"))])).unwrap();
    pump_until(&mut [&mut good], |ps| ps[0].pending_len() == 0);
    let health = rt.block_on(running.hub.health()).unwrap();
    assert_eq!(health.connections, 1);
    assert_eq!(health.head, 1);
    let body = ureq::get(&format!("{}/healthz", running.url())).call().unwrap().into_string().unwrap();
    assert!(body.contains("connections 1") && body.contains("head 1"), "{body}");
    rt.block_on(running.stop());
}

#[test]
fn a_peer_in_this_process_needs_no_socket() {
    let rt = runtime();
    let running = serve(&rt, None, |b| b);
    let mut scanner = Peer::open_memory(demo::domain(), Options::dev("library").with_timing(quick())).unwrap();
    scanner.connect_with("local", running.hub.dial());
    let mut bob = peer(&running, "bob");
    scanner.mutate("create_playlist", args([("name", Value::text("Found"))])).unwrap();
    pump_until(&mut [&mut scanner, &mut bob], |ps| ps[0].pending_len() == 0 && ps[1].cursor() == 1);
    let rows = rt.block_on(running.hub.rows("playlist")).unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0]["user_id"], Value::text("library"));
    rt.block_on(running.stop());
}
