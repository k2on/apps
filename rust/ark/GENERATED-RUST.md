# Generated Rust against `ark`

What `arkc gen rust` writes compiles against `use ark::gen::*;`. This file
lists every place the Rust spelling departs from `spec/GENERATED.md`, and
the choices made where that contract is silent. Beyond snake case and `?`
there should be nothing here that a reader of GENERATED.md would not
expect; each line below is a candidate contract amendment.

## Spelling

| the contract says | Rust |
|---|---|
| `Value.null()`, `Value.bool(b)`, `Value.int(i)`, `Value.text(s)` | `Value::null()`, `Value::bool(b)`, `Value::int(i)`, `Value::text(s)` (`s: impl Into<String>`) |
| `Value.bytesHex("0a0b")`, `Value.idHex("…")` | `Value::bytes_hex("0a0b")`, `Value::id_hex("…")` (dashes tolerated) |
| bytes and ids from typed data (not in the contract) | `Value::bytes(Vec<u8>)`, `Value::id(Id)` where `Id = [u8; 16]` |
| an option (not in the contract) | `Value::opt(Option<Value>)` — flat, `None` is `Null` |
| `Value.list([v…])` | `Value::list(vec![v…])` |
| `Value.record([("k", v)…])` | `Value::record(vec![("k".to_string(), v)…])` (any `Into<String>` key) |
| `v.isNull()`, `v.asBool()`, `v.asInt()` | `v.is_null()`, `v.as_bool()`, `v.as_int()` |
| `v.asText()` | `v.as_text()` → `&str` |
| `v.asList()` | `v.as_list()` → `Vec<Value>` (owned, so `for x in xs.as_list()` binds a `Value`) |
| `v.field("k")` | `v.field("k")` → `Value` (owned) |
| `Fault.refuse(t)`, `Fault.bug(t)` | `Fault::refuse(t)` (`t: impl Reason` — a `&str`, a `String` or a text `Value`), `Fault::bug(t)`; a fault is `Err(Fault)` |
| `Ops.add(a,b)` … `Ops.neg(a)` | `Ops::add(a, b)?` … `Ops::neg(a)?`, by value, `Result<Value, Fault>` |
| `Ops.mod(a,b)` | `Ops::r#mod(a, b)?` — `mod` is a keyword |
| `Ops.cmp(CmpOp.Lt, a, b)` | `Ops::cmp(CmpOp::Lt, a, b)` → `Value` (cannot fault, no `?`) |
| `Ops.not(a)` | `Ops::not(a)` → `Value` (a non-bool is fatal, as `as_bool` is) |
| `Ops.arg(args, "name")` | `Ops::arg(&args, "name")` → `Value`, cloned; missing is fatal (a bug, as an accessor mismatch is) |
| `Ops.match(opt, some, none)` | `Ops::match_opt(opt, \|v\| -> Result<Value, Fault> {…}, \|\| -> Result<Value, Fault> {…})?` — `match` is a keyword |
| `Ops.map/filter/any/all/sortBy/fold` | `Ops::map(xs, f)?` … `Ops::sort_by(xs, key)?`, `Ops::fold(xs, init, f)?`; closures are `FnMut(Value) -> Result<Value, Fault>` (`fold`: `FnMut(Value, Value)`) |
| `and` / `or` | `Value::bool(a.as_bool() && b.as_bool())` — the language's short circuit |
| `Std.trim(a)` … `Std.unwrapOr(a, d)` | `Std::trim(a)?` … `Std::unwrap_or(a, d)?`, by value, all `Result<Value, Fault>` |
| `db.get(table, [key…])` | `db.get("table", vec![key…])` → `Value` (the row or `Value::null()`) |
| `db.exists(table, [key…])` | `db.exists("table", vec![key…])` → `Value::bool` |
| `db.select(plan)` | `db.select(&plan)` → `Value::list` |
| `db.put(table, row)`, `db.delete(table, [key…])` | `db.put("table", row)?`, `db.delete("table", vec![key…])?` → `Result<(), Fault>`, the fault being the store's refusal |
| `Plan.from(t).filter(p).orderBy(c, Dir.Asc).limit(n).related(…)` | `Plan::from("t").filter(p).order_by("c", Dir::Asc).limit(n).related("name", "parent", "child", "column", child_plan)` |
| `Pred.cmp(col, CmpOp.Eq, v)`, `Pred.inList`, `all`, `any`, `not` | `Pred::cmp("col", CmpOp::Eq, v)`, `Pred::in_list("col", vec![…])`, `Pred::all(vec![…])`, `Pred::any(vec![…])`, `Pred::not(p)` |
| `ctx.user`, `ctx.session` as `Value.text` | `Ctx { pub user: String, pub session: String }`; generated code spells `Value::text(ctx.user.clone())` |
| `Args` | `type Args = BTreeMap<String, Value>`; `Args::from([("k".to_string(), v)])` |

## Choices the contract does not make

- **`db` is `ark::gen::Db`**, an overlay over any `&dyn Store` that records
  what it changed. A generated mutator is `fn m(db: &mut Db, ctx: &Ctx,
  autos: &Args, args: &Args) -> Result<(), Fault>`; a query is `fn q(db:
  &Db, args: &Args) -> Result<Value, Fault>`. `ark::gen::run_mutator(store,
  |db| …)` runs a body as one transaction, exactly as `apply_closure` does:
  changes committed on `Ok`, nothing on a verdict, and a `Fault::Refuse`
  that came from the store is handed back as that store's structured
  `Refusal` (`Db::verdict`).
- **What is fatal and what is a fault.** The accessors, `Ops::arg`,
  `Ops::not` and a non-literal plan handed to `Store::select` panic with a
  message naming the bug, as GENERATED.md asks of the accessors; everything
  a verified module can legitimately reach faults with `Err(Fault)`.
  `Std` and `Ops` report a type mismatch as `Fault::Bug` rather than
  panicking, because they take `Value`s a closure may have produced.
- **`Store::select` takes literal plans only.** Generated code evaluates
  every right-hand side before building the plan, so the store's `select`
  refuses (fatally) any `Expr` that is not `Expr::Lit`. The interpreter has
  its own `select` that evaluates expressions.
- **Table names are `&str`, keys are `Vec<Value>`**, on `Db`; the `Store`
  trait underneath takes `&[Value]`.
- The module is `ark::stdlib`, not `ark::std`, so that `std::` paths stay
  unambiguous inside the crate; the unit struct is `Std` as the contract
  spells it.
