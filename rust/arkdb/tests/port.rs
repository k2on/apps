//! `arkdb export` and `arkdb import` over servers' data directories: a
//! running server's rows out in their human form, transformed, and in again
//! under a schema that grows from the one they were read under.

use std::path::Path;
use std::sync::Arc;

use ark::format::Encode;

use ark_peer::{Config, Server};
use arkdb::demo::{demo, CreatePlaylist, Playlist};
use serde_json::Value as Json;

fn dir(name: &str) -> std::path::PathBuf {
    let d = std::env::temp_dir().join(format!("arkdb-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    d
}

fn server(data: &Path) -> ark_peer::Handle {
    let mut s = Server::new(Config::of(Arc::new(demo())));
    s.data = Some(data.to_path_buf());
    s.build().unwrap().0
}

fn args(a: &[&str]) -> Vec<String> {
    a.iter().map(|s| s.to_string()).collect()
}

/// A server's playlists, exported while it serves, every name upper-cased
/// by hand, and imported into an empty directory: a server opened there
/// holds them so. An import under a schema that does not grow from the
/// export's is refused. Falsified by the server not keeping its schema
/// beside its log: "no schema.json".
#[test]
fn a_servers_rows_are_exported_transformed_and_imported() {
    let (from, to) = (dir("from"), dir("to"));
    let hub = server(&from);
    for name in ["Hymns", "Psalms"] {
        let input = CreatePlaylist { name: name.into() }.to_bytes();
        hub.with(move |p| p.mutate_at("demo.create_playlist", input, 1)).unwrap().unwrap();
    }
    let export = arkdb::port::export(&args(&["--from", &from.display().to_string()])).unwrap();
    hub.stop();
    let mut export: Json = serde_json::from_str(&export).unwrap();
    for p in export["tables"]["playlist"].as_array_mut().unwrap() {
        p["name"] = Json::String(p["name"].as_str().unwrap().to_uppercase());
    }
    let file = to.with_extension("json");
    std::fs::write(&file, export.to_string()).unwrap();
    let schema = to.with_extension("schema.json");
    std::fs::write(&schema, serde_json::to_vec(&*demo().schema).unwrap()).unwrap();
    let (f, s, t) = (file.display().to_string(), schema.display().to_string(), to.display().to_string());
    arkdb::port::import(&args(&[&f, &s, "--to", &t])).unwrap();
    let hub = server(&to);
    let names = hub
        .with(|p| ark::db::read::<arkdb::demo::Demo, _>(&p.log.store, |db| db.playlist.scan()))
        .unwrap();
    let mut names: Vec<String> = names.into_iter().map(|p: Playlist| p.name).collect();
    names.sort();
    assert_eq!(names, ["HYMNS", "PSALMS"]);
    hub.stop();
    let mut narrower = (*demo().schema).clone();
    narrower.tables[0].columns.pop();
    std::fs::write(&schema, serde_json::to_vec(&narrower).unwrap()).unwrap();
    let again = dir("again").display().to_string();
    let refused = arkdb::port::import(&args(&[&f, &s, "--to", &again])).unwrap_err();
    assert!(refused.contains("does not grow"), "{refused}");
    for d in [from, to] {
        let _ = std::fs::remove_dir_all(d);
    }
}
