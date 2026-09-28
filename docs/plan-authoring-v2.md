# Plan: finish authoring v2 (routers, checks, printers) and the harken work

Written so that a session on any model can pick this up mid-way. Read
`spec/AUTHORING.md` first — it is the contract every step below serves —
then this file top to bottom. Nothing here asks for a design decision;
where one was open it has been made and is recorded in AUTHORING.md.

## Ground rules (from the user, do not relax)

- Commit only to `k2on/apps` on `main`; never to `petros` or `harken`.
- Every commit ends with exactly these two lines, nothing else after them:
  `Co-Authored-By: Claude Fable 5.1 <noreply@anthropic.com>` and
  `Claude-Session: https://claude.ai/code/session_014Nz5smRBnkQqoe6xuVGUvk`.
- No model identifiers anywhere else (code, comments, docs, PR text).
- Never write the user's name or email into anything.
- Everything builds with nix. No wasm-interpreted domains, no Rust macros
  in the authoring API (a domain is plain functions and closures).
- Commit and push at every stable point; the stop hook refuses to end a
  turn with uncommitted work. Commit only your own files by path — four
  background agents have uncommitted work of their own in the tree (see
  "Agents" below); do not `git add -A`.

## Environment notes

- nix is used as `export PATH=/nix/var/nix/profiles/default/bin:$PATH
  NIX_SSL_CERT_FILE=/root/.ccr/ca-bundle.crt` before every command. If
  the daemon is missing, start `nix-daemon` with that cert variable.
- Build the spec with `cd spec && nix develop ..#spec -c cabal build`;
  run the vectors with `cabal run -v0 ark-vectors -- <outdir>` and diff
  against `spec/vectors`; `nix flake check` at the root runs everything.
- The disk is a fixed allowance. When it fills, `nix store gc` frees the
  dead paths (16 GB last time); `rust/target` is the other large thing.
- `fetchgit` through GitHub fails in this sandbox (proxy CA); `fetchurl`
  works; plain `git clone` of public repositories works.

## State at the time of writing (commit `fbff26a`)

Done, in the spec (`spec/src/Ark/*.hs`), building and with the vectors
regenerated at spec version 2:

- `IR.hs`: `Router`, `modRouters`, `fnRouter`, `fnUses`, `Guard`/`Provide`,
  `Field`/`Check`, `fnInput`/`fnRefine`, `SInsert`/`SUpsert`/`SUpdate`,
  `EProvided`, `StdFn.Unwrap`, `specVersion = 2`.
- `Encode.hs`/`Decode.hs`: Appendix A's wire form, including `"uses"`.
- `Store.hs`: `insertOn`, `upsertOn`, `update`, `matchOn`.
- `Verify.hs`: every rule in AUTHORING §1.5 plus the ones its header lists.
- `Eval.hs`: checks → middleware in `fnUses` order → body; `check` (the
  form validator); `defaultMessage`; queries take a `Ctx` now.
- `Hash.hs`: closures and `deps` reach through middleware.
- `Demo.hs`: Appendix B's demo (scope `demo`, `playlist`/`item`).
- `Print.hs`: the diagnostic form knows every new node.
- `app/Vectors.hs`: regenerated; new `eval/checks.json` and
  `eval/form-check.json`; every rebase/fleet/view assertion still holds.
- `harken/domain/src/{schema,library,playlists,module}.rs`: the canonical
  Rust text, already in the derived-names form (§6).

Not done: `Ark.Gen` is **removed from `ark-spec.cabal`** and the `gen`
command from `app/Arkc.hs` (the old runtime-code generator no longer
compiles against v2 and is being replaced, not fixed). `flake.nix`'s
`harken-domain` package and the `check-generated`-style checks still call
`arkc gen … --name Harken --only …`, so `nix flake check` is red until
step 2 lands. `spec/GENERATED.md` and `spec/generated/` are stale.

## Steps, in order

### 1. Message the agents (2 minutes)

Send each of the three runtime agents the two rules that landed after
their last message, if they have not acknowledged them: (a) `.first()`
emits two `SLet`s itself and a host `let` emits nothing; (b) names are
derived by the printer (AUTHORING §6 "Names are derived, never
captured"), so `Emit` may leave `fnNames` empty and the canonical harken
text now says `playlist_item.map_or(0, |row| row.pos)` and
`.filter(|row| …)`. Agent ids: Rust `a14c0d139945f55dc`, Swift
`a32e8bac8ccc257e9`, Kotlin `a4a2f26c2d868f6db`, Web `a633c23831fbfd1b9`.

### 2. Replace `spec/src/Ark/Gen.hs` with the authoring-form printers

One module, `Ark.Gen`, exporting `Target (..)`, `printSchema`,
`printRouter`, `printModuleFile`, `files :: Target -> Options -> Module ->
[(FilePath, Text)]`, and `restrict`. It writes AUTHORING §2.5 and §6
exactly. Work from the canonical files, which are the acceptance test:

- Rust must reproduce `harken/domain/src/{schema,library,playlists,module}.rs`
  byte for byte after comment-only lines are removed, and Appendix B's
  demo from `Ark.Demo.demoModule`.
- Swift and Kotlin follow the same layout in their spellings (§2.1–2.5,
  and the Kotlin paragraph under "Files" in §6 — the Kotlin agent's
  `dev.arkdb.authoring` package is the reference; ask it for one printed
  file if in doubt and match it).

Layout rules the Rust files already exhibit (write them once as helpers):

- A method chain goes on one line when the whole statement fits in 150
  columns (`rustfmt` `max_width = 150`, see `harken/domain/rustfmt.toml`);
  otherwise it breaks before every `.` with one extra indent, the
  receiver alone on the first line (`db.track` then `.insert(…)` then
  `.on(…)`; `db` then `.playlist_item` then `.filter(…)` … when the
  receiver chain itself is long — copy what `playlists.rs` does).
- A struct literal breaks one field per line when it does not fit.
- `routes((` … `))` holds one route per entry, each `router.input::<I>()
  .mutation("name", |ctx, db, input| { … })` (or `.query`, or with the
  provided parameter after `input`), the closure body indented four,
  closed by `}),`.
- Unused parameters are `_ctx`, `_db`, `_input`; a route with no input
  writes `_input: ()`.
- Orders print the shortest prefix whose key-completion is the stored
  order; the `on` list prints as a tuple of `Row::col`; keys as tuples.
- `let <table> = <read>;` is printed for every read except: the read that
  is a query's returned value (`SLet s (ESelect); SReturn (EVar s)` prints
  as the chain ending in `.all()`), and the `or_refuse` shape (`SLet s
  get; SIf (IsSome s) [] [SRefuse m]; … Unwrap s`) which prints as
  `.or_refuse("m")` on the chain. A `first()` pair prints as one `let`.
- Names: AUTHORING §6. Keep a counter per table name for the `_2` suffix.

Also add `arkc gen rust|swift|kotlin M OUTDIR [--package p] [--only f,g]`
and `arkc roundtrip rust|swift|kotlin M SRC_DIR` to `app/Arkc.hs`
(roundtrip: gen into a temp dir under `System.Directory
getTemporaryDirectory`, strip comment-only lines from both sides, diff,
exit 1 on any difference, print the first differing file and line).
Put `Ark.Gen` back in `ark-spec.cabal`. Then:

    cd spec && nix develop ..#spec -c cabal run -v0 arkc -- demo /tmp/d.ark
    cabal run -v0 arkc -- gen rust /tmp/d.ark /tmp/gen-rust
    # compare by eye with AUTHORING.md Appendix B, then
    cabal run -v0 arkc -- roundtrip rust harken/domain/harken.ark harken/domain/src

`harken/domain/harken.ark` is the OLD module (spec 1) — regenerate it from
the Rust agent's `module().emit()` once that builds (it is what the Rust
agent's `harken-domain` binary writes); until then hand-check the demo
only. Falsify the roundtrip once: change one name in a canonical file
and see it fail.

### 3. Vectors and the spec's own checks

- `app/Vectors.hs`: add `module/demo-print-rust.txt`-style vectors? No —
  the printed forms are checked by `roundtrip`, not by vectors. Do add
  one `verify/` falsify vector: the demo with `create_playlist`'s `on`
  set to `["name"]` (not a unique index) must fail with `OnNotUnique`;
  and one with `fnUses = ["nope"]` failing `NotMiddleware`. Write them as
  `verify/falsify/*.json` with `"verifies": false` and the complaint name.
- Regenerate `spec/vectors` (`rm -rf spec/vectors && cp -r <out>
  spec/vectors`) and commit.

### 4. Docs

- Delete `spec/GENERATED.md` and `spec/generated/`; every reference to
  them (`README.md`, `spec/README.md`, `docs/arkdb.md` §3.x, the three
  runtime READMEs) points at `spec/AUTHORING.md` instead. `spec/README.md`
  is the section index: add §3.10 routers, §3.11 input checks, §4.3a–c
  the three writes, §6.7 the validator, §6.8 default messages, §18 the
  printers and roundtrip. Keep the prose in the style the files already
  have (one paragraph per decision, why before what).
- `docs/arkdb.md`: one new section, "Authoring: one vocabulary, three
  spellings", that states the round-trip property and points at
  AUTHORING.md; strike the sentences that say a generator writes native
  code from the IR.

### 5. Integration, once the four agents report

Each agent's final report says what it changed and what it could not
verify. Then, in this order:

1. `nix flake check` must be made green:
   - `flake.nix`: `harken-domain` builds the Rust agent's binary, runs
     `module().emit()` to `harken.ark`, then `arkc verify` and
     `arkc roundtrip rust harken.ark harken/domain/src`; the Swift and
     Kotlin roundtrips run against the files the agents authored
     (`swift/…/HarkenDomain/*.swift`, `kotlin/…/harken/domain/*.kt` —
     take the paths from their reports). Drop every `gen … --only` use.
   - `check-vectors` unchanged. The three runtimes' conformance tests
     must pass the regenerated vectors, including `eval/checks.json`
     and `eval/form-check.json`; each runtime also holds `emit(demo)`
     to `module/demo.json`'s bytes (§5 check 1) and `Native` against the
     eval vectors (§5 check 3).
   - `packages.harken-web` and the two workflows from the Web agent.
2. `harken.ark` committed fresh; `harken/domain/gen/` and `gen-check`
   deleted (nothing is generated for a runtime any more).
3. `README.md` and `harken/README.md`: the module/procedure story, the
   Pages URL `https://k2on.github.io/apps/harken/`, the APK workflow.
4. Commit and push. Then report to the user: what is verified from here
   (spec, vectors, Rust workspace, whatever built under nix) and what is
   not (a phone, a browser).

## Agents

Four Opus subagents were launched by this session and are still running;
they deliver their reports as task notifications. Their briefs, in one
line each:

- Rust (`a14c0d139945f55dc`): `rust/ark` IR v2 + `ark::authoring`
  (Native/Emit behind one API), `ark-builder`/`gen` deleted,
  `harken/domain` compiling as the canonical files, server and desktop
  on `module().procedures()`, an agreement test Native vs the eval vectors.
- Swift (`a32e8bac8ccc257e9`): `swift/` IR v2 + `ArkAuthoring` module,
  harken's domain authored in Swift, the iOS app on it.
- Kotlin (`a4a2f26c2d868f6db`): `kotlin/` IR v2 + `dev.arkdb.authoring`,
  harken's domain in Kotlin, the Android app; `nix build .#harken-apk`
  must still pass.
- Web (`a633c23831fbfd1b9`): `harken/web` (Rust→wasm peer + TS UI),
  `packages.harken-web`, `.github/workflows/pages.yml` deploying to
  `k2on.github.io/apps/harken`, `.github/workflows/apk.yml`. It is the
  only agent allowed to edit `flake.nix`; it was waiting on a nix build
  when it last reported.

Decisions already sent to them (all also in AUTHORING.md): per-procedure
`uses`; the §6 lowerings; `Unwrap`/`or_refuse`; derived names; the
relation const named after the child table; the printer strips the
order's key completion; Kotlin's header and spellings; Swift's
`import ArkAuthoring`.

If an agent asks a question the contract answers, point it at the
section. If it asks one the contract does not answer, decide, write the
answer into AUTHORING.md first, then reply — the file is the source of
truth, the messages are not.
