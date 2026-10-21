//! The `views/` vectors' plans as JSON (`ark::dynamic`): each scenario's
//! query, as a runner in any language reads it.

#![allow(dead_code)]

use serde_json::{json, Value as Json};

fn v(path: &str) -> Json {
    json!({ "var": path })
}

fn eq(a: Json, b: Json) -> Json {
    json!({ "==": [a, b] })
}

fn related(table: &str, on: &str, to: Json) -> Json {
    json!({ "related": table, "on": on, "to": to })
}

fn sort(input: Json, var: &str, by: Json) -> Json {
    json!({ "sort": input, "as": var, "by": by })
}

fn map(input: Json, var: &str, to: Json) -> Json {
    json!({ "map": input, "as": var, "to": to })
}

fn filter(input: Json, var: &str, cond: Json) -> Json {
    json!({ "filter": input, "as": var, "where": cond })
}

fn scan(t: &str) -> Json {
    json!({ "scan": t })
}

fn take(input: Json, n: usize) -> Json {
    json!({ "take": input, "n": n })
}

fn desc(k: Json) -> Json {
    json!({ "desc": k })
}

fn len(e: Json) -> Json {
    json!({ "len": e })
}

/// `(how many movements, title)` of each work by title, then id.
fn works_with_counts(with_id: bool) -> Json {
    let mut each = vec![len(related("movement", "work_id", v("w.id"))), v("w.title")];
    if with_id {
        each.push(v("w.id"));
    }
    map(sort(scan("work"), "w", json!([v("w.title"), v("w.id")])), "w", Json::Array(each))
}

/// `(key, how many, the sum of column c)` per group.
fn counted(input: Json, by: &str, col: &str) -> Json {
    json!({
        "group": input, "as": "r", "by": v(&format!("r.{by}")), "key": "k", "members": "ms",
        "to": [v("k"), len(v("ms")), { "sum": map(v("ms"), "m", v(&format!("m.{col}"))) }]
    })
}

/// The plan of the vector `name`.
pub fn plan(name: &str) -> Json {
    match name {
        "top-two-by-pos" => take(
            sort(
                filter(scan("item"), "i", eq(v("i.playlist_id"), json!({ "arg": "playlist_id" }))),
                "i",
                json!([v("i.pos"), v("i.playlist_id"), v("i.track_id")]),
            ),
            2,
        ),
        "playlist-with-items" => map(
            sort(scan("playlist"), "p", json!([v("p.name"), v("p.id")])),
            "p",
            json!([
                v("p.id"),
                v("p.name"),
                v("p.user_id"),
                take(
                    sort(
                        related("item", "playlist_id", v("p.id")),
                        "i",
                        json!([desc(v("i.pos")), v("i.playlist_id"), v("i.track_id")])
                    ),
                    3
                )
            ]),
        ),
        "projection" => map(
            sort(scan("song"), "s", json!([v("s.pos"), v("s.id")])),
            "s",
            json!([v("s.pos"), v("s.title")]),
        ),
        "having-admits-on-a-child" => filter(works_with_counts(false), "x", json!({ ">": [v("x.0"), 0] })),
        "group-source" => sort(
            json!({ "group": scan("song"), "as": "s", "by": v("s.creator"), "key": "k", "members": "ms", "to": len(v("ms")) }),
            "g",
            json!([v("g.0")]),
        ),
        "lookup-chain" => {
            let none = json!(["", v("s.title"), ""]);
            let found = json!({
                "let": { "w": { "get": "work", "key": v("m.work_id") } },
                "in": [v("m.part"), v("s.title"), { "??": [v("w.title"), ""] }]
            });
            let each = json!({
                "let": { "m": { "get": "movement", "key": v("s.movement_id") } },
                "in": { "if": [eq(v("m"), Json::Null), none, found] }
            });
            map(sort(scan("song"), "s", json!([v("s.pos"), v("s.id")])), "s", each)
        }
        "related-on-a-non-key-column" => map(scan("person"), "p", json!([len(related("song", "creator", v("p.name"))), v("p.name")])),
        "expression-order-under-a-limit" => map(
            take(sort(works_with_counts(true), "x", json!([desc(v("x.0")), v("x.2")])), 2),
            "x",
            json!([v("x.0"), v("x.1")]),
        ),
        "related-three-deep" => {
            let movement = json!([
                v("m.id"),
                v("m.work_id"),
                v("m.no"),
                v("m.part"),
                related("song", "movement_id", v("m.id"))
            ]);
            let work = json!([
                v("w.id"),
                v("w.composer"),
                v("w.title"),
                map(related("movement", "work_id", v("w.id")), "m", movement)
            ]);
            map(
                scan("person"),
                "p",
                json!([v("p.name"), v("p.born"), map(related("work", "composer", v("p.name")), "w", work)]),
            )
        }
        "playlist-join-item" => json!({ "join": [scan("playlist"), scan("item")], "as": ["p", "i"], "on": [v("p.id"), v("i.playlist_id")] }),
        "kept-max" => {
            let last = json!({ "first": take(sort(related("track", "creator", v("p.name")), "t", json!([desc(v("t.pos"))])), 1) });
            map(
                scan("person"),
                "p",
                json!({ "let": { "t": last }, "in": [{ "??": [v("t.pos"), -1] }, v("p.name")] }),
            )
        }
        "has-search" => {
            let has = json!({ "contains": [{ "lower": v("t.title") }, { "lower": { "arg": "needle" } }] });
            map(
                sort(filter(scan("track"), "t", has), "t", json!([v("t.pos"), v("t.id")])),
                "t",
                json!([v("t.pos"), v("t.title")]),
            )
        }
        "a-maintained-sum-answers-where-the-fold-overflows" => {
            map(sort(counted(scan("t1"), "c12", "k1"), "g", json!([desc(v("g.0"))])), "g", v("g.1"))
        }
        "a-maintained-sum-overflows-where-the-fold-answers" => {
            let other = filter(scan("t0"), "r", json!({ "!=": [v("r.p0"), { "arg": "f_p0" }] }));
            map(filter(counted(other, "c00", "n0"), "g", json!({ ">=": [v("g.1.1"), 2] })), "g", v("g.1"))
        }
        n => panic!("no plan {n}"),
    }
}
