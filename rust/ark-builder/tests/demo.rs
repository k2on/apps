//! The demo domain of `spec/src/Ark/Demo.hs`, written through the builder,
//! must come out as the bytes and the hash `spec/vectors/module/demo.json`
//! records — the strongest proof there is that the builder emits what the
//! spec means, since that vector is the verified form of the Haskell
//! module: orders completed, symbols renumbered.

use std::fs;
use std::path::Path;

use ark::value::hex;
use ark_builder::*;

fn demo() -> ModuleBuilder {
    let mut m = ModuleBuilder::new();
    m.scope("playlists", |s| {
        s.table("playlist", |t| {
            t.id("id");
            t.text("name");
            t.text("user_id");
            t.key(&["id"]);
        });
        s.table("playlist_item", |t| {
            t.id_ref("playlist_id", "playlist");
            t.bytes("media_id");
            t.int("pos");
            t.int("added_ms");
            t.text("user_id");
            t.key(&["playlist_id", "media_id"]);
        });
    });
    m.mutator("create_playlist", "playlists", |f| {
        let id = f.new_id("id", "playlist");
        let name = f.arg("name", Ty::Text);
        let b = f.body();
        b.if_(name.trim().is_empty(), |b| b.refuse("a playlist needs a name"));
        let known = b.exists("playlist", [id.clone()]);
        b.if_(known, |b| b.ret());
        b.put("playlist", record([("id", id), ("name", name.trim()), ("user_id", ctx_user())]));
    });
    m.mutator("add_to_playlist", "playlists", |f| {
        let added_ms = f.now("added_ms");
        let playlist_id = f.arg("playlist_id", Ty::id("playlist"));
        let media_id = f.arg("media_id", Ty::Bytes);
        let b = f.body();
        let has_playlist = b.exists("playlist", [playlist_id.clone()]);
        b.if_(has_playlist.not(), |b| b.ret());
        let already = b.exists("playlist_item", [playlist_id.clone(), media_id.clone()]);
        b.if_(already, |b| b.ret());
        let rows = b.select(
            Plan::from("playlist_item")
                .filter(Pred::cmp("playlist_id", CmpOp::Eq, playlist_id.clone()))
                .order_by("pos", Dir::Desc)
                .limit(1),
        );
        // Demo.hs spells MAX(pos) + 1 as `unwrapOr (match (first rows) r (Some r.pos) (None Int)) 0`.
        let last = b.let_("last", rows.first().map_some(|r| r.field("pos")).or_none(Ty::Int).unwrap_or(0));
        b.put(
            "playlist_item",
            record([
                ("playlist_id", playlist_id),
                ("media_id", media_id),
                ("pos", last.add(1)),
                ("added_ms", added_ms),
                ("user_id", ctx_user()),
            ]),
        );
    });
    m
}

fn vector() -> serde_json::Value {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../spec/vectors/module/demo.json");
    serde_json::from_str(&fs::read_to_string(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()))).unwrap()
}

#[test]
fn the_demo_hashes_as_the_spec_says() {
    let v = vector();
    let m = demo();
    assert_eq!(hex(&m.bytes()), v["bytes"].as_str().unwrap(), "canonical bytes");
    assert_eq!(m.hash(), v["hash"].as_str().unwrap(), "module hash");
}

#[test]
fn the_builder_writes_what_it_hashes() {
    let m = demo();
    let module = m.build();
    assert_eq!(hex(&ark::ir::module_hash(&module)), m.hash());
    // Names are the builder's, never the file's: the decoded module has none.
    let back = ark::ir::module_from_value(&ark::canon::decode(&m.bytes()).unwrap()).unwrap();
    assert_eq!(ark::ir::module_value(&module), ark::ir::module_value(&back));
    assert_eq!(hex(&ark::ir::module_hash(&back)), m.hash());
}
