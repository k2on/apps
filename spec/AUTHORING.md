# Writing a domain in Rust

A domain is a crate of plain Rust (`docs/plan-v5.md` §1.16): its rows,
its inputs and its functions, registered by name in a `Domain` that the
kernel runs. There is one runtime. A peer finds a function by the name an
entry carries, and code never crosses the wire. `rust/ark-dev/src/demo.rs`
is the demo, and `harken/domain/src/` is a whole application, one file
per part. Everything below comes from `ark::prelude::*`.

## Rows and the schema

```rust
#[derive(Clone, Debug, PartialEq, Eq, Default, Serialize, Deserialize, Row)]
#[ark(index(playlist_id, pos))]
pub struct PlaylistItem {
    #[ark(key, refs(Playlist))]
    pub playlist_id: Id<Playlist>,
    #[ark(key, refs(Media))]
    pub media_id: Id<Media>,
    pub pos: i64,
    pub added_ms: i64,
    pub user_id: String,
}

#[derive(Schema)]
pub struct Harken { pub playlist: Playlist, pub playlist_item: PlaylistItem, /* … */ }
```

`#[derive(Row)]` makes a table out of a struct. Its fields are the
columns, in order. The leading `#[ark(key)]` fields are the key, and a
table has at least one.

- `index(a, b)` and `unique(a, b)` declare an index, a constant named
  after its columns (`PlaylistItem::PLAYLIST_ID_POS`; `as NAME` renames).
- `refs(T)` on a column makes it name a row of `T`. An `Option` column
  names one only when it is `Some`.
- `#[ark(volatile)]` on the struct puts the table in live state (§1.9). Its
  rows are never logged or hashed.

`#[derive(Schema)]` on the struct of tables makes the schema: its tables
in field order, named by the fields. Grow a schema only by appending
tables, and columns to the end of a table. An appended column is an
`Option` or carries `#[serde(default)]`. The schema is data: it is hashed,
carried in every `Hello`, prefix-checked when a log opens, and read by the
explorer.

A column is an `i64`, `String`, `bool`, `Vec<u8>`, `Id<T>`, `Option`,
`Vec`, tuple, or a record or enum deriving `Describe`; no floats. A row is
its JSON (serde), and a key the same value memcomparable (§1.1).

## Inputs

```rust
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, Input)]
pub struct CreatePlaylist {
    #[ark(trim, len = "1..=80", why = "a playlist needs a name")]
    pub name: String,
}
```

`#[derive(Input)]` writes `check`: the form's validation, run before a
mutator sees the input, its refusal the intent's. A field's refinements
apply in order: `trim` normalises it for the rules after it; `non_empty`,
`len = "lo..=hi"` and `range = "lo..=hi"` bound it; `check = "path::fn"`
runs a predicate. `why = "…"` replaces the default words (`at least 1
character`, `must not be empty`). Every failing field is reported with its
words, which is what a form shows.

## Functions

```rust
#[mutator("playlists.add_to_playlist")]
pub fn add_to_playlist(ctx: &Ctx, db: &mut Db<Harken>, input: AddToPlaylist) -> Result<(), Refusal> {
    let playlist = owned(ctx, db, input.playlist_id)?;
    if db.media.has(input.media_id) && !db.playlist_item.has((playlist.id, input.media_id)) {
        let pos = last_pos(db, playlist.id).unwrap_or(0) + 1;
        db.playlist_item.insert(PlaylistItem { playlist_id: playlist.id, media_id: input.media_id, pos, added_ms: ctx.now, user_id: ctx.user().into() })?;
    }
    Ok(())
}

fn last_pos(db: &Db<Harken>, playlist: Id<Playlist>) -> Option<i64> {
    db.playlist_item.by(PlaylistItem::PLAYLIST_ID_POS).from(playlist).last().map(|i| i.pos)
}
```

The five kinds of function:

- **A mutator** is `fn(&Ctx, &mut Db<S>, I) -> Result<(), Refusal>`. The
  machine runs it as an intent. A refusal undoes every write it made, so a
  run applies all of its writes or none.
- **A live transition** (`#[live]`) is a mutator over volatile tables.
- **A query** (`#[query]`) is `fn(&Ctx, &Db<S>, I) -> Result<T, Refusal>`,
  answered once.
- **A view** (`#[view]`) returns `Result<Plan<S, T>, Refusal>`. It is kept
  up to date, and a UI holds its patches.
- **A scope** (`#[scope]`) returns a `Scope<S>`: per table, a
  `fn(&Ctx, &R) -> bool` that says which rows a connection fed by it holds
  (§1.8).

A helper or a guard takes `&Db<S>` and cannot write: the borrow checker
is the effect system.

The attribute only registers the function by its name, and
`ark::register!(Domain::new::<Harken>("harken", VERSION); playlists::create_playlist, …)`
lists them.

**Determinism is an API rule.** A run is a function of the store, its
input and its `Ctx` (`user`, `session`, `roles`, `now`, `device`).
`ctx.now` and `ctx.new_id()` are a function's only non-determinism, and
both are frozen in the entry. A function reads no clock, no randomness and
nothing of its host. When replicas disagree, the authority's facts win.

## Reads and writes

- `db.t.get(key)` and `db.t.has(key)` read one row (a key of several
  columns is a tuple); `db.t.scan()` reads every row, in key order.
- `db.t.by(T::INDEX)` reads by an index: `.from(prefix)`, `.between(lo,
  hi)` or `.whole()`, then `.first()`, `.last()`, `.take(n)`, `.all()`,
  `.count()` or `.any()`; `.rev()` reads from the end.
- `insert`, `update`, `put` (insert or update), `delete` (by key; a
  missing row is no change) and `remove` (by row) are the writes. Each is a
  `Change` the store judges as it is made (a key taken, a unique index, a
  reference), so a function reads what it wrote.

A read goes exactly where it says, by the key or a declared index, so a
wrong column does not compile and nothing routes a read at run time.

## Views

```rust
#[view("playlists.playlist")]
pub fn playlist(ctx: &Ctx, db: &Db<Harken>, input: PlaylistInput) -> Result<Plan<Harken, LibraryEntry>, Refusal> {
    let playlist = owned(ctx, db, input.playlist_id)?;
    Ok(scan::<Harken, PlaylistItem>()
        .filter_with(move |db, i| i.playlist_id == playlist.id && db.media.has(i.media_id))
        .sort(|i| i.pos)
        .map_with(|db, i| library_entry(&db.media.get(i.media_id).expect("an item's media"), Some(i.pos))))
}
```

A plan is `scan`, then `filter`, `map`, `sort` (by a key; `Desc(k)` for
descending), `take(n)`, `group(by, agg)` and `join`. The `_with` forms
read the store, as lookups and related rows do. Each operator is kept per
element. A change re-runs a closure only for the elements whose recorded
reads it hits. A view's own reads, here the playlist's owner, rebuild it
when they move. After every change, the view's answer is what a fresh run
of the plan gives, and its patches splice the old list into the new.

Rows without a Rust type (the explorer, the `views/` vectors) use the same
plans over JSON values, written as JSON (`ark::dynamic`).
