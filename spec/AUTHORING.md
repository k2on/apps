# ArkLang

`docs/plan-v5.md` §1.15. A domain is written in ArkLang; the module
(§1.5) — what runtimes load, check, hash and call by name — is built from
the text, and the text is the source of truth. ArkLang is S-expressions:
one reader rule, `(head args…)`, so there is one spelling by
construction, the printer is a tree walk, and a reader in any language is
small. One printer: `build(print(m)) == m` for every module, and
`print(build(t)) == t` for every form as the printer lays it out.

The reference is `rust/ark/src/sexp.rs` (reader, lowering, printer,
`build_dir`, `load`); `arkc` (`rust/ark-dev`) checks, prints, compares
and compiles modules; `rust/ark-dev/src/demo.ark` is the demo and
`harken/domain/*.ark` a whole application.

## 1. Reading

Atoms are **symbols**, **integers** (`5`, `-5`), **strings** `"…"` (`\"`
and `\\` the only escapes) and **bytes** `0x00ff`; `true`, `false` and
`none` are symbols with a meaning. Lists are `( … )`; `;` comments run to
the end of a line. A symbol may hold `.`: one whose head is a bound
variable is a field path (`input.playlist_id`, `pair.0`); otherwise it is
a declared name (`playlists.owned`, `Clock.Paused`). An error is said at
its `file:line:column`.

**The layout.** A form is one line when it ends by column 100, the `)`s
closing on that line counted. Otherwise its head stays on the first line
with its first argument when that is an atom or ends by column 100 there
(a function's name, an `if`'s condition, a `let`'s bindings), and every
other argument is on its own line, two spaces in from the form's `(`. A
list whose head is no symbol — a `let`'s bindings — is one element a line,
each under the first. Top-level forms are a blank line apart. Comments
are not part of the module: the printer writes none, and a file is its
forms printed once its comments are set aside.

## 2. How a domain is laid out

A domain is a **directory**: every `*.ark` file under it, at any depth,
is read in sorted path order, all of its forms first, and only then are
names resolved — so neither the order of forms nor which file holds one
means anything. Files are not namespaces; dotted names are
(`library.add_song`, whose prefix is its router). Several `(schema …)`
forms are one schema, their tables in file and form order. A name — a
table's, a type's, a function's — declared twice is refused with both
places. `arkc print MODULE -o DIR` writes a module back as one file per
dotted prefix beside `schema.ark` and `types.ark`; an author splits files
as the reading goes, as `harken/domain` does (`keys.ark`, `queue.ark`).

## 3. Top-level forms

```
(schema (table NAME [volatile] (COL TY [key] [(refs TABLE)] [(default 0x…)])… (index [unique] COL…)…)…)
(type NAME TY)
(mutator NAME ((P TY)…) BODY…)          ; Write, returns ()
(live    NAME ((P TY)…) BODY…)          ; Write over volatile tables only, returns ()
(query   NAME ((P TY)…) RET BODY…)      ; Read
(helper  NAME ((P TY)…) RET BODY…)      ; the least effect its body has
(guard   NAME ((P TY)…) RET BODY…)      ; as helper
(scope   NAME ((ROW TY)) BODY…)         ; Pure, returns Bool; TY the sum of the tables' rows
```

A table's key is its leading `key` columns (the first when none is
marked). A body of several forms is a sequence whose value is the last.
Every function is bound by its name except one whose name starts with
`_`. The printer writes the kind by the effect: `live` when every table
reached is volatile, else `mutator` for `Write` returning `()`; `query`
for `Read`; `scope` for a `Pure` predicate of the rows' sum; `helper`
otherwise.

## 4. Types

| type | is |
|---|---|
| `Int` `Bytes` `Text` `Bool` `Never` | the leaves; `Text = Bytes where utf8`; `Bool` the sum of two empty variants; `Never` the empty sum |
| `(id TABLE)` | `Bytes where len 16`, declared as `Id<TABLE>` |
| `(option TY)` | `Sum [(), (TY)]`, declared as `Option<TY>` |
| `(list TY)`, `(tuple TY…)` | a list; a tuple (`(tuple)` is `()`) |
| `(row TABLE)` | the table's row: the tuple of its columns |
| `(record (F TY)…)` | at the top of a `type`: a tuple whose fields are named, the names kept in the declaration; a field `(F (where …))` declares `NAME.F` |
| `(sum (V (F TY)…) (W))` | at the top of a `type`: a sum whose variants and their fields are named |
| `(where TY RULE ["words"]…)` | a refinement: `(utf8)` `(trim)` `(non-empty)` `(len N)` `(len LO HI)` `(len LO ..)` `(fn (x) e)`, each followed by the words a form shows when a value breaks it |

## 5. Expressions

| form | IR |
|---|---|
| `5`, `"x"`, `0x00ff`, `true`, `false`, `(lit TY 0x…)` | `Lit` |
| `()` | `Seq([])`, unit |
| `(let ((X E)…) BODY…)`; `(let () E E…)` a sequence | `Let`, `Seq` |
| `(if C A [B])` — no `B` is `()` | `If` |
| `(match E (V (X…) BODY…)…)` — every variant in order, each binding all its fields; `V` the declaration's name, `none`/`some`, `false`/`true`, or a position | `Match` |
| `x.f.g`, `(. E f g)` | `Field` |
| `(record (F E)…)` where the place wants a record or a row, `(tuple E…)` elsewhere; `(as (row T) (record …))` a row no place wants | `Tuple` |
| `(T.V E…)` a declared sum's variant; `(variant TY I E…)` any | `Variant` |
| `(some E)`, `none` (where the place wants an option), `(list E…)`; `(as TY e)` gives one of these the type `TY` | `Variant`, `List` |
| `(?? P D)` — `P` a path through an option, `o.f` or `(. E f)` | `(match o (none () D) (some (x) x.f))` |
| `(?. E f…)` | `(match E (none () none) (some (x) (some x.f…)))` |
| `(?? E D)` — `E` an option no path goes through; `(some? E)`, `(none? E)` | `Std(UnwrapOr)`, `Std(IsSome)`, `not` of it |
| `(== a b)` `!=` `<` `<=` `>` `>=` `+` `-` `*` `/` `%` `and` `or`, `(not E)`, `(- E)` | `Op`, `Un` |
| `(refuse "msg")` | `Refuse` |
| `(fn (X…) BODY)` — only as an operator's argument | `Lambda` |
| `TABLE`, `(get T KEY…)`, `(filter L FN)`, `(map L FN)`, `(join A B FN)`, `(group L FN FN)`, `(sort L (fn (x) KEYS))` with `(desc E)`, `(asc E)`, `(tuple k (desc k2))`, `(take L N)` | `Scan` `Lookup` `Filter` `Map` `Join` `Group` `Sort` `Take` |
| `(insert T REC)`, `(update T KEY… FN)`, `(delete T KEY…)` | `Insert` `Update` `Delete` |
| `ctx.user` `ctx.session` `ctx.roles` `ctx.now` `(new-id T)` `(has-role "x")` | `Ctx`, `Std(Contains, [ctx.roles, x])` |
| `(NAME ARG…)` a declared function | `Call(hash, …)` |

A table's key is its key columns as arguments (`(get playlist_item
playlist.id media_id)`), or one expression of the key's tuple type. A
binder shadows a table of its name. The names an author gives binders are
the function's (`Function::names`, in the order a walk of the body meets
them), so the printer writes them back; a function built without them is
printed with names made from the binders' types.

## 6. The standard functions

`std.rs`'s, by their names with `-` for `_`; each keeps the refinements
its signature says (a function of text returns `Text`, its argument's
length bound dropped; `reverse` and `move-at` keep a list's length rules,
`insert-at` and list `concat` keep `non-empty`).

| function | takes | returns |
|---|---|---|
| `trim` `lower` | `Text` | `Text` |
| `is-empty` `is-alnum` | `Text` | `Bool` |
| `concat` | `(list Text)` | `Text` |
| `chars` | `Text` | `(list Text)` |
| `len` | `Text` (code points) or a list | `Int` |
| `fnv1a64` | `Text` | `Int` |
| `starts-with` | `Text`, `Text` | `Bool` |
| `split-once` | `Text`, a separator `Text` | `(option (tuple Text Text))` |
| `to-text` | `Int` | `Text` |
| `hex` `text-of-id` | `Bytes` | `Text` |
| `id-of-text` | `Text` | `(option Bytes)` |
| `min` `max`; `clamp`; `abs` | `Int`s | `Int` |
| `sha256`; `nil-id`; `utf8` | `Bytes`; nothing; `Text` | `Bytes` |
| `first` `last` | a list | an option of its element |
| `contains` | a list, an element | `Bool` |
| `reverse`; `concat` (two lists); `insert-at`; `remove-at`; `move-at`; `slice` | a list, and indices or a list | a list |
| `some?` | an option | `Bool` |
| `??` (`unwrap_or`); `unwrap` | an option and a default; an option | its value (`unwrap` of none refuses) |
| `sum` | `(list Int)` | `Int`, judged once for overflow |

## 7. Typing

**Every expression has a type, and a declared type is a subtype of what it
refines.** `Name ≤ Text ≤ Bytes`; a value becomes refined only by a
literal, parsed once under its type, or by a function whose signature
says so. Comparison is of one structure, refinements aside.

```
(helper short ((name Name)) Text (trim name))      ; Name ≤ Text, and trim returns Text
```

**A `none`, a `(record …)` and a `(list)` take the type their place
wants** — a function's result, a branch's, a field's, an argument's, a
row's, a `map`'s element — and `(as T e)` gives one where no place does.

```
(query owner ((id (id playlist))) (option Text) (?. (get playlist id) user_id))
```

## 8. Effects

**Every expression has an effect, `Pure < Read < Write`, and a function
may have no more than its kind says.** A `query` reads, a `mutator`
writes, a `scope` and a refinement's predicate are pure; a `helper` or a
`guard` has the least its body has; a `live` function writes volatile
tables only. A body evaluates to the changes it makes, and reads through
them.

```
(query names () (list Text) (map playlist (fn (p) p.name)))   ; Read: a scan
```

A call is by hash, to a function built before, so the call graph is a DAG
and every function ends; a lambda stands only as an operator's argument;
`ctx.now` is the entry's, frozen.

## 9. Bindings

`ark::gen::rust(module)` writes a host's Rust for a module: types and
stubs, no function body. An app's `build.rs` calls
`ark::gen::build_script(dir)`, which builds the domain's directory and
writes `OUT_DIR/domain.rs` (the bindings and `MODULE`, the module file's
bytes) where rust-analyzer sees them. It emits:

- `HASH`, the module's hash, and `SHAPE`, what the bindings were made
  from: its schema, its declarations, each function's parameters and
  result;
- a struct per table's row (`PlaylistItem`), per record (`AddSong`), an
  enum per sum (`Clock::Paused { at_ms }`), an alias per refinement or
  other name (`Queue = Vec<Track>`); `Id<Row>`, `Option`, `Vec`, `String`,
  `i64`, `bool` for the rest; each derives `Node`, `Clone`, `Debug`,
  `PartialEq`, `Eq`, and `Copy` and `Default` where its fields allow;
- a stub per bound function, in a module per dotted prefix, typed by its
  parameters (named as the author named them) and its result: a mutator
  or a live function `fn add_to_playlist<C: Call>(peer: &mut C, input:
  &AddToPlaylist) -> C::Authored`, a query or helper `fn playlists<C:
  Call>(peer: &C) -> Result<Vec<Playlist>, String>`; each encodes its
  arguments, calls the function by its binding through `Call`, and decodes
  the answer;
- `fits(&Domain)`: whether a loaded module is one the bindings may call —
  its schema a prefix-extension of theirs (§1.2), every type and function
  they name of the structure they were made with, a row grown by appended
  columns read by its prefix (`ark::gen::answer`, `ark::gen::rows`).

`Call` is implemented by whatever runs the module — a replica, a hub, the
domain alone over an empty store (`impl Call for Domain`).
