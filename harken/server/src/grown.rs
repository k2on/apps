//! **Test-only.** harken's domain, grown the way a domain is allowed to
//! grow (§17, `docs/plan-db.md` D1 scenario 6): what the fleet's version
//! scenarios run as "the next release" while no pinned revision of this
//! repository has a different schema.
//!
//! Three changes over [`harken_domain::module`], each the shape a real one
//! would take:
//!
//! - **a nullable column**: `playlist_item.note`, written by
//!   `set_item_note` and read by the query `item_notes` — on a table no
//!   base function returns whole, since a function that does declares the
//!   row's type and would have to move with it;
//! - **a new table**: `tag`, a playlist's labels, written by `tag_playlist`;
//! - **a mutator whose body moved**: `create_playlist` with one more
//!   whole-input check that always holds — the same behaviour, a new hash,
//!   which is what a release that touches a mutator ships. A peer of the
//!   base module authors it at the old hash, and only closure provenance
//!   lets the grown server run that.
//!
//! Selected by the flag both binaries already have for hosting another
//! module: `HARKEN_MODULE=FILE` for `harken-server`, `--module FILE` for
//! `harken-peer`, with `FILE` what [`write`] writes. Nothing in a
//! deployment reaches it: it is compiled into the library so that the
//! fleet can write the file, and it is never a default.
//!
//! The base module's own procedures stay native wherever their hash did
//! not move; `create_playlist` and the grown functions run through the
//! interpreter, as any function of a module loaded from a file without a
//! procedure does.

use std::path::Path;

use anyhow::{anyhow, Context, Result};
use ark::authoring::*;
use ark::ir::{self, Expr};
use ark::schema::{Column, Table as TableDecl, Ty};
use ark::value::Value;
use ark_client::Domain;
use harken_domain::schema as base;

/// The grown tables, as the grown functions read them: harken's own
/// `playlist` and `media`, `playlist_item` with its new column, and `tag`.
/// Only for authoring those functions; the module's tables are harken's
/// declarations with `note` added and `tag` after them ([`module`]).
pub struct Grown {
    pub playlist: Table<base::Playlist>,
    pub media: Table<base::Media>,
    pub playlist_item: Table<PlaylistItem>,
    pub tag: Table<Tag>,
}
impl Tables for Grown {
    fn open() -> Self {
        Grown {
            playlist: table(),
            media: table(),
            playlist_item: table(),
            tag: table(),
        }
    }
}

/// harken's playlist item, with a note.
pub struct PlaylistItem {
    pub playlist_id: Id<base::Playlist>,
    pub media_id: Id<base::Media>,
    pub pos: Int,
    pub added_ms: Int,
    pub user_id: Text,
    pub note: Opt<Text>,
}
impl Row for PlaylistItem {
    const NAME: &str = "playlist_item";
    type Key = (Id<base::Playlist>, Id<base::Media>);
    fn columns() -> Columns<Self> {
        columns()
            .id(Self::playlist_id)
            .refs::<base::Playlist>()
            .id(Self::media_id)
            .refs::<base::Media>()
            .int(Self::pos)
            .int(Self::added_ms)
            .text(Self::user_id)
            .text(Self::note)
            .nullable()
            .key((Self::playlist_id, Self::media_id))
    }
}
#[allow(non_upper_case_globals)]
impl PlaylistItem {
    pub const playlist_id: Col<Self, Id<base::Playlist>> = col("playlist_id");
    pub const media_id: Col<Self, Id<base::Media>> = col("media_id");
    pub const pos: Col<Self, Int> = col("pos");
    pub const added_ms: Col<Self, Int> = col("added_ms");
    pub const user_id: Col<Self, Text> = col("user_id");
    pub const note: Col<Self, Opt<Text>> = col("note");
}

/// A label on a playlist: the new table.
pub struct Tag {
    pub playlist_id: Id<base::Playlist>,
    pub tag: Text,
}
impl Row for Tag {
    const NAME: &str = "tag";
    type Key = (Id<base::Playlist>, Text);
    fn columns() -> Columns<Self> {
        columns()
            .id(Self::playlist_id)
            .refs::<base::Playlist>()
            .text(Self::tag)
            .key((Self::playlist_id, Self::tag))
    }
}
#[allow(non_upper_case_globals)]
impl Tag {
    pub const playlist_id: Col<Self, Id<base::Playlist>> = col("playlist_id");
    pub const tag: Col<Self, Text> = col("tag");
}

pub struct SetNote {
    pub playlist_id: Id<base::Playlist>,
    pub media_id: Id<base::Media>,
    pub note: Text,
}
impl Input for SetNote {
    fn schema() -> Object<Self> {
        object()
            .field("playlist_id", id::<base::Playlist>().exists())
            .field("media_id", id::<base::Media>())
            .field("note", text())
    }
}

pub struct Label {
    pub playlist_id: Id<base::Playlist>,
    pub tag: Text,
}
impl Input for Label {
    fn schema() -> Object<Self> {
        object()
            .field("playlist_id", id::<base::Playlist>().exists())
            .field("tag", text().min(1))
    }
}

pub struct Items {
    pub playlist_id: Id<base::Playlist>,
}
impl Input for Items {
    fn schema() -> Object<Self> {
        object().field("playlist_id", id::<base::Playlist>())
    }
}

/// The grown functions, as a module of their own: the two new mutators,
/// and a query that reads the new column.
fn extra() -> Module {
    let r = router::<Grown>("grown");
    Module::new((r.routes((
        r.input::<SetNote>()
            .mutation("set_item_note", |_ctx, db, input| {
                db.playlist_item
                    .update((input.playlist_id, input.media_id), |p| PlaylistItem {
                        playlist_id: p.playlist_id,
                        media_id: p.media_id,
                        pos: p.pos,
                        added_ms: p.added_ms,
                        user_id: p.user_id,
                        note: some(input.note),
                    })
            }),
        r.input::<Label>()
            .mutation("tag_playlist", |_ctx, db, input| {
                db.tag.insert(Tag {
                    playlist_id: input.playlist_id,
                    tag: input.tag,
                })
            }),
        // A playlist's items as stored, the note among them.
        r.input::<Items>().query("item_notes", |_ctx, db, input| {
            db.playlist_item
                .filter(PlaylistItem::playlist_id.eq(input.playlist_id))
                .order_by(PlaylistItem::pos.asc())
        }),
    )),))
}

/// The grown module, verified.
pub fn module() -> ir::Module {
    let mut m = harken_domain::module().build().clone();
    let extra = extra();
    let more = extra.build();
    for t in &mut m.schema.tables {
        if t.name == "playlist_item" {
            let mut columns = t.columns.clone();
            columns.push(Column {
                name: "note".into(),
                ty: Ty::Text,
                nullable: true,
            });
            *t = TableDecl::new(
                t.name.clone(),
                columns,
                t.key.clone(),
                t.indexes.clone(),
                t.refs.clone(),
            )
            .with_text(t.text.clone());
        }
    }
    m.schema.tables.extend(
        more.schema
            .tables
            .iter()
            .filter(|t| t.name == "tag")
            .cloned(),
    );
    for f in &mut m.functions {
        if f.name == "create_playlist" {
            f.refine
                .push((Expr::Lit(Value::Bool(true)), Some("grown".into())));
        }
    }
    m.functions.extend(more.functions.iter().cloned());
    m.routers.extend(more.routers.iter().cloned());
    ark::verify::verify(&m).unwrap_or_else(|es| panic!("the grown module does not verify: {es:?}"))
}

/// The grown module as a peer or the server holds it: every procedure of
/// harken's whose hash did not move native, and the grown ones.
pub fn domain() -> Domain {
    let mut procs = harken_domain::module().procedures();
    procs.extend(extra().procedures());
    Domain::of(module(), procs)
}

/// The grown module's canonical bytes, as an `.ark` file at `path`: what
/// `HARKEN_MODULE` and `harken-peer --module` read.
pub fn write(path: &Path) -> Result<()> {
    let bytes = ark::canon::encode(&ir::module_value(&module()));
    std::fs::write(path, bytes).with_context(|| format!("writing {}", path.display()))?;
    Domain::from_bytes(&std::fs::read(path)?, vec![])
        .map(|_| ())
        .map_err(|e| anyhow!("the grown module read back: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// harken's module with a nullable `playlist.note` and nothing else: a
    /// column on a table that `playlists`, `playlists_of` and the `owned`
    /// middleware return or provide whole.
    fn noted_playlist() -> ir::Module {
        let mut m = harken_domain::module().build().clone();
        for t in &mut m.schema.tables {
            if t.name == "playlist" {
                let mut columns = t.columns.clone();
                columns.push(Column {
                    name: "note".into(),
                    ty: Ty::Text,
                    nullable: true,
                });
                *t = TableDecl::new(
                    t.name.clone(),
                    columns,
                    t.key.clone(),
                    t.indexes.clone(),
                    t.refs.clone(),
                )
                .with_text(t.text.clone());
            }
        }
        m
    }

    /// A row in a function's signature is the table's row (§9, `docs/plan-db.md`
    /// D1): the module with `playlist.note` added verifies, and every
    /// function that names a whole playlist keeps the hash it had —
    /// `playlists` among them — because nothing in it moved.
    ///
    /// Falsified once: with `verify`'s query-result comparison exact again,
    /// the module refused `playlists`' declared result, one field short.
    #[test]
    fn a_nullable_column_moves_no_function_that_returns_its_rows() {
        let base = Domain::new(&harken_domain::module());
        let noted = ark::verify::verify(&noted_playlist()).unwrap_or_else(|es| panic!("{es:?}"));
        let noted = Domain::of(noted, vec![]);
        assert_ne!(base.hash(), noted.hash(), "the module moved");
        for name in [
            "playlists",
            "playlists_of",
            "owned",
            "create_playlist",
            "add_to_playlist",
        ] {
            let (h, _) = base.function(name).unwrap();
            assert_eq!(
                noted.function(name).map(|(h, _)| h),
                Some(h),
                "{name} kept its hash"
            );
        }
    }

    /// The grown module is the base one plus the column, the table and the
    /// two mutators — every base function but `create_playlist` at the hash
    /// it had, and `create_playlist` at another.
    ///
    /// Falsified once: without the check added to `create_playlist`, its
    /// hash was the base one and the assertion named it.
    #[test]
    fn grown_is_harken_grown() {
        let base = Domain::new(&harken_domain::module());
        let grown = domain();
        let tbl = grown.module().schema.lookup_table("playlist_item").unwrap();
        assert!(tbl.column("note").is_some_and(|c| c.nullable));
        assert!(grown.module().schema.lookup_table("tag").is_some());
        for (h, c) in base.closures() {
            let kept = grown.closures().contains_key(h);
            assert_eq!(
                kept,
                c.function.name != "create_playlist",
                "{}",
                c.function.name
            );
        }
        assert!(grown.mutator("set_item_note").is_ok() && grown.mutator("tag_playlist").is_ok());
        assert!(grown.query("item_notes").is_ok());
        assert_ne!(base.hash(), grown.hash());
    }
}
