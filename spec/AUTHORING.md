# Writing a domain

A domain is a crate of plain Rust (`docs/plan-v5.md` §1.16): its rows,
its inputs and its functions, registered by name in a `Domain` the kernel
runs. There is one runtime; a peer finds a function by the name an entry
carries, and code never crosses the wire. Everything below comes from
`ark::prelude::*`. Every example is cut from `rust/arkdb/src/demo.rs`
or `harken/domain/src/` (a line `// …` skips some), and
`rust/arkdb/tests/authoring.rs` holds each to its source.

## Rows and the schema

```rust
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, Row)]
#[ark(unique(playlist_id, pos))]
pub struct Item {
    #[ark(key, refs(Playlist))]
    pub playlist_id: Id<Playlist>,
    #[ark(key)]
    pub track_id: String,
    pub pos: i64,
}

#[derive(Schema)]
pub struct Demo {
    pub playlist: Playlist,
    pub item: Item,
}
```

`#[derive(Row)]` makes a table of a struct: its fields are the columns, in
order, and the leading `#[ark(key)]` fields the key (at least one).
`index(a, b)` and `unique(a, b)` declare an index, a constant named after
its columns (`Item::PLAYLIST_ID_POS`; `as NAME` renames). `refs(T)` makes
a column name a row of `T` (an `Option` only when `Some`); the store
refuses a write that breaks it, and a delete of a row still named.
`#[ark(volatile)]` puts a table in live state (§1.9): never logged or
hashed. A scope (§1.8) feeds a table whole unless `#[ark(owner = user)]`
(its person's rows) or `#[ark(unfed)]` (none). `#[derive(Schema)]` makes
the schema, tables in field order.

A column is an `i64`, `String`, `bool`, `Vec<u8>`, `Id<T>`, `Option`,
`Vec`, a tuple, or a record or enum deriving `Describe`; no floats. A row
is its bytes in the value format, a key the same value memcomparable
(§1.1). A schema grows only by appending tables, and columns to the end
of a table, each an `Option` or `#[serde(default)]`; it is hashed, said in
every `Hello`, prefix-checked when a log opens.

## Inputs

```rust
#[derive(Clone, Debug, PartialEq, Eq, Default, Serialize, Deserialize, Input)]
pub struct CreatePlaylist {
    #[ark(trim, non_empty, why = "a playlist needs a name")]
    #[ark(len = "0..=120")]
    pub name: String,
}
```

`#[derive(Input)]` writes `check`, the form's validation, run before a
mutator sees the input; its refusal is the intent's. A field's
refinements apply in order: `trim` normalises it for the rules after it;
`non_empty`, `len = "lo..=hi"` and `range = "lo..=hi"` bound it;
`check = "path::fn"` runs a predicate; `why = "…"` replaces the default
words (`at least 1 character`, `must not be empty`).

## Functions

```rust
#[mutator("demo.add_to_playlist")]
pub fn add_to_playlist(_: &Ctx, db: &mut Db<Demo>, input: AddToPlaylist) -> Result<(), String> {
    if !db.playlist.has(input.playlist_id) {
        return Err("playlist_id: no such playlist".into());
    }
    // …
    let last = db.item.by(Item::PLAYLIST_ID_POS).from(input.playlist_id).last();
    db.item.insert(Item {
        playlist_id: input.playlist_id,
        track_id: input.track_id,
        pos: last.map_or(0, |i| i.pos) + 1,
    })
}
```

- **A mutator** is `fn(&Ctx, &mut Db<S>, I) -> Result<(), String>`, run
  as an intent; a refusal undoes every write it made. Over `Db<S,
  Volatile>` it writes the live space's tables (§1.9) and its intents
  are on that space: a peer's `mutate` sends each to the space its
  function writes.
- **A view** (`#[view]`) returns a `Plan<S, T>`, kept up to date or
  answered once (as a job's `due` is).
- **A scope** (`#[scope]`) says per table which rows a connection fed by
  it holds (§1.8); a table it does not name, as the row says.

A helper or a guard takes `&Db<S>` and cannot write: the borrow checker
is the effect system. The attribute names a function and writes beside it
its typed handle (`ADD_TO_PLAYLIST: Func<AddToPlaylist>`), which Rust calls
it by; `functions!` registers each one its file and modules declare (not under `#[cfg]`):

```rust
#[scope("demo.top_two")]
pub fn top_two() -> Scope<Demo> {
    Scope::new().on::<Item>(|_, i| i.pos <= 2)
}

/// The demo, every function registered.
pub fn demo() -> Domain {
    ark::functions!(Domain::new::<Demo>("demo", "1"))
}
```

**Determinism is an API rule.** A run is a function of the store, its
input and its `Ctx` (`user`, `session`, `roles`, `now`, `device`).
`ctx.now` and `ctx.new_id()` (the entry's `n`-th id) are its only
non-determinism, both frozen in the entry. It reads no clock, randomness
or host; when replicas disagree anyway, the authority's facts win.

## Reads and writes

`db.t.get(key)` and `db.t.has(key)` read one row (a key of several
columns a tuple); `db.t.scan()` every row in key order; `db.t.by(T::IX)`
an index, `.from(prefix)`, `.between(lo, hi)` or `.whole()`, then
`.first()`, `.last()`, `.take(n)`, `.all()`, `.count()` or `.any()`,
`.rev()` from the end. A read goes where it says, so a wrong column does
not compile. `insert`, `update`, `put`, `delete` (by key) and `remove`
(by row) are the writes, each a `Change` the store judges as it is made,
so a function reads what it wrote.

## Views

```rust
#[view("playlists.playlist")]
pub fn playlist(ctx: &Ctx, db: &Db<Harken>, input: PlaylistInput) -> Result<Plan<LibraryEntry>, String> {
    let playlist = owned(ctx, db, input.playlist_id)?;
    Ok(scan_from::<Harken, PlaylistItem, _, _>(playlist.id)
        .filter_with(|db, i| db.media.has(i.media_id))
        .sort(|i| i.pos)
        .map_with(|db, i| library_entry(&db.media.get(i.media_id).expect("an item's media"), Some(i.pos))))
}
```

A plan is `scan` (or `scan_from(k)`, the rows whose key begins with `k`,
read from there), then `filter`, `map`, `sort` (`Desc(k)` descending),
`take(n)`, `group(by, agg)` and `join`; the `_with` forms read the store.
Each operator is kept per element, and a change re-runs a closure only
for the elements whose recorded reads it hits; the view's own reads (the
playlist's owner) rebuild it when they move. After every change the
answer is a fresh run's and the patches splice the old list into the new.
Rows without a Rust type (the `views/` vectors, a host speaking JSON) use the
same plans over JSON, written as JSON (`ark::dynamic`).
