#![forbid(unsafe_code)]
//! What `arkc gen rust` wrote from `harken.ark`, compiled against
//! `ark::gen` exactly as it was written — the file is included by path,
//! never copied — and, under test, run beside the interpreter so that the
//! two agree on what every mutator does.

// The generator writes the file; rustfmt does not get to rewrite it.
#[rustfmt::skip]
#[path = "../../gen/rust/harken_gen.rs"]
pub mod harken;

#[cfg(test)]
mod tests {
    use ark::canon::decode;
    use ark::eval::apply;
    use ark::gen::*;
    use ark::hash::state_hash;
    use ark::ir::{module_from_value, module_hash, Module};
    use ark::store::MemoryStore;
    use ark::value::{decode_hex, hex};

    use super::harken::*;

    fn module() -> Module {
        let bytes = decode_hex(MODULE_BYTES).unwrap();
        let m = module_from_value(&decode(&bytes).unwrap()).unwrap();
        assert_eq!(hex(&module_hash(&m)), MODULE_HASH, "the generated file names the module it came from");
        m
    }

    #[test]
    fn generated_code_agrees_with_the_interpreter() {
        let m = module();
        let ctx = Ctx::new("alice", "session-1");
        let mut fast = MemoryStore::empty(m.schema.clone());
        let mut slow = fast.clone();

        // A track, twice: the second is the rescan and writes nothing.
        let track = Value::id_hex("00000000-0000-0000-0000-000000000010");
        let autos = Args::from([("id".to_string(), track.clone()), ("added_ms".to_string(), Value::int(1))]);
        // A client is generated without add_track (only the scanner authors
        // it), so a track reaches a client as facts; here the interpreter
        // stands in for the server's application on both stores.
        let args = Args::from([
            ("title".to_string(), Value::text(" Air ")),
            ("artist".to_string(), Value::text("Bach")),
            ("album".to_string(), Value::null()),
            ("duration_ms".to_string(), Value::int(300_000)),
            ("file".to_string(), Value::text("music/bach/air.flac")),
        ]);
        for _ in 0..2 {
            let a = apply(&m, "add_track", &ctx, &autos, &args, &mut fast).unwrap().unwrap();
            let b = apply(&m, "add_track", &ctx, &autos, &args, &mut slow).unwrap().unwrap();
            assert_eq!(a, b);
        }
        assert_eq!(fast.scan("track").len(), 1);

        // A playlist, then the same track on it twice: pos 1, then a no-op.
        let pid = Value::id_hex("00000000-0000-0000-0000-000000000001");
        let autos = Args::from([("id".to_string(), pid.clone()), ("created_ms".to_string(), Value::int(2))]);
        let args = create_playlist_args(" Favorites ".into());
        let a = run_mutator(&mut fast, |db| create_playlist(db, &ctx, &autos, &args)).unwrap().unwrap();
        let b = apply(&m, "create_playlist", &ctx, &autos, &args, &mut slow).unwrap().unwrap();
        assert_eq!(a, b);
        let now = Args::from([("added_ms".to_string(), Value::int(3))]);
        let args = add_to_playlist_args(pid.as_id(), track.as_id());
        for _ in 0..2 {
            let a = run_mutator(&mut fast, |db| add_to_playlist(db, &ctx, &now, &args)).unwrap().unwrap();
            let b = apply(&m, "add_to_playlist", &ctx, &now, &args, &mut slow).unwrap().unwrap();
            assert_eq!(a, b);
        }
        assert_eq!(fast, slow);
        assert_eq!(hex(&state_hash(&fast)), hex(&state_hash(&slow)));

        // The reads, through the generated queries.
        let db = Db::new(&fast);
        let items = query("playlist_items", &db, &Args::from([("playlist_id".to_string(), pid.clone())]))
            .unwrap()
            .as_list();
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].field("pos"), Value::int(1));
        assert_eq!(library(&db, &Args::new()).unwrap().as_list()[0].field("title"), Value::text("Air"));
        assert_eq!(playlists(&db, &Args::new()).unwrap().as_list()[0].field("name"), Value::text("Favorites"));

        // And off again, both ways.
        let a = run_mutator(&mut fast, |db| remove_from_playlist(db, &ctx, &Args::new(), &args))
            .unwrap()
            .unwrap();
        let b = apply(&m, "remove_from_playlist", &ctx, &Args::new(), &args, &mut slow).unwrap().unwrap();
        assert_eq!(a, b);
        assert!(fast.scan("playlist_item").is_empty());
        // Six of the domain's seven: a client is generated without add_track.
        assert_eq!(FUNCTIONS.len(), 6);
    }
}
