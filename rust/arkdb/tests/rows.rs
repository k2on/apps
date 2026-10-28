//! `arkdb rows` over a server's data directory: the rows a running server
//! wrote, read under the schema it keeps beside its log, while it runs.

use std::sync::Arc;

use ark::format::Encode;
use ark::schema::Space;
use ark_peer::{Config, Server};
use arkdb::demo::{demo, CreatePlaylist};

/// A server's playlists, read from its directory as it serves: only those
/// with the name asked, newest last first, and the first `-n`. Falsified by
/// the server not keeping its schema beside its log: "no schema.json".
#[test]
fn a_servers_rows_are_read_where_it_keeps_them() {
    let dir = std::env::temp_dir().join(format!("arkdb-rows-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let mut server = Server::new(Config::of(Arc::new(demo())));
    server.data = Some(dir.clone());
    let (hub, _) = server.build().unwrap();
    for name in ["Hymns", "Psalms", "Carols"] {
        let input = CreatePlaylist { name: name.into() }.to_bytes();
        hub.with(move |p| p.author(Space::Durable, "demo.create_playlist", input, 1))
            .unwrap()
            .unwrap();
    }
    let rows = |args: &[&str]| {
        let mut a: Vec<String> = args.iter().map(|s| s.to_string()).collect();
        a.extend(["--from".into(), dir.display().to_string()]);
        arkdb::rows::main(&a)
    };
    let names = |out: String| -> Vec<String> {
        out.lines()
            .map(|l| {
                serde_json::from_str::<serde_json::Value>(l).unwrap()["name"]
                    .as_str()
                    .unwrap()
                    .to_string()
            })
            .collect()
    };
    assert_eq!(names(rows(&["playlist", "--sort", "-name"]).unwrap()), ["Psalms", "Hymns", "Carols"]);
    assert_eq!(names(rows(&["playlist", "name=Psalms"]).unwrap()), ["Psalms"]);
    assert_eq!(rows(&["playlist", "-n", "1"]).unwrap().lines().count(), 1);
    assert_eq!(rows(&["nothing"]), Err("no table nothing".into()));
    hub.stop();
    let _ = std::fs::remove_dir_all(&dir);
}
