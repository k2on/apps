//! The maintained library says exactly what the read one says, and the list
//! a screen holds says exactly what the view says: hydrate once, push every
//! change, splice the patches — after every step, equal to `library` run
//! again. What a client depends on, and nothing else notices if it drifts.

mod common;

use ark::store::Store;
use ark::value::Value;
use ark::view;
use common::{args, Lib, Song};
use harken_domain::view::{entry_of, library_plan, patch, Item};

#[test]
fn the_maintained_library_agrees_with_the_read_one() {
    let mut c = Lib::new();
    let favs = c.playlist("alice", "Favorites");
    let other = c.playlist("alice", "Other");
    let schema = c.store.schema().clone();
    let plan = library_plan(favs);
    let mut v = view::hydrate(&schema, &plan, &c.store);
    let mut shown: Vec<Value> = v.rows().iter().map(entry_of).collect();
    let mut patched = 0;

    let mut step = |c: &mut Lib, name: &str, a: ark::eval::Args| {
        c.mutate("alice", name, a).unwrap_or_else(|e| panic!("{name}: {e}"));
        for ch in c.last.clone() {
            let (next, ps) = view::push(&schema, &c.store, &ch, &v);
            v = next;
            patched += ps.len();
            patch(&mut shown, &ps);
        }
        assert!(view::contract(&schema, &plan, &c.store, &v), "{name}: the view is a fresh hydrate");
        assert_eq!(shown, c.library(favs), "{name}: the spliced list is the query");
    };

    for (t, f) in [("Glue", "a"), ("Opal", "b"), ("Gosh", "c"), ("Air", "d")] {
        step(
            &mut c,
            "add_song",
            Song {
                file: f.into(),
                ..Song::new(t, "X", "Y")
            }
            .args(),
        );
    }
    let ids: Vec<Value> = c.library(favs).iter().map(|r| r.field("id")).collect();
    let on = |p, m: &Value| args([("playlist_id", Value::Id(p)), ("media_id", m.clone())]);
    step(&mut c, "add_to_playlist", on(favs, &ids[2]));
    step(&mut c, "add_to_playlist", on(favs, &ids[0]));
    // Another playlist's entries are not this view's business.
    step(&mut c, "add_to_playlist", on(other, &ids[1]));
    step(&mut c, "remove_from_playlist", on(favs, &ids[2]));
    step(&mut c, "add_all_to_playlist", args([("playlist_id", Value::Id(favs))]));
    step(&mut c, "remove_media", args([("id", ids[0].clone())]));
    step(
        &mut c,
        "add_song",
        Song {
            file: "e".into(),
            ..Song::new("Late", "Z", "")
        }
        .args(),
    );
    assert!(patched > 0);

    let items: Vec<Item> = shown.iter().map(Item::from_value).collect();
    assert_eq!(
        items.iter().map(|i| (i.title.as_str(), i.playlist_pos)).collect::<Vec<_>>(),
        [("Opal", Some(3)), ("Gosh", Some(4)), ("Air", Some(5)), ("Late", None)]
    );
    assert!(items[0].on_playlist() && !items[3].on_playlist());
}
