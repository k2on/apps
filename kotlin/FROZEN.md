# Frozen at spec v4

This tree is ArkDB for Kotlin at **spec version 4** — `ark-runtime`,
`ark-client`, `ark-ui`, `ark-android` and `harken-peer` — and the phone
(`harken/android`) is built on it. Spec v5 (`docs/plan-v5.md`) replaced
v4 in Rust, and v4's reference, vectors and module are gone from the tree
(`rust/ark`, `spec/` and `harken/domain` are v5's now), so nothing here
can be checked against the tree as it is.

It was last recorded green at **`a983b3c`**, the last commit to change
it, whose message records its conformance runs passing over that
commit's `spec/vectors`. The v4 inputs its check reads moved once after,
at `35c2f79` (`harken/domain/harken.ark` and the three `harken-*` traces),
with no Kotlin run recorded; `8bda5c3` is the last commit with v4 in the
tree. From the repository root,

    nix build "git+file://$PWD?rev=$(git rev-parse a983b3c)#checks.x86_64-linux.kotlin"

builds the check as it passed, and `#harken-apk` at the same revision the
phone.

**Its checks have left the flake**: `checks.kotlin` and
`checks.fleet-kotlin`, with `packages.arkdb-kotlin`, `harken-peer-kt`,
`kotlin-deps`, `harken-apk`, `harken-android-screens`,
`harken-fdroid-repo`, `aapt2` and `android-sdk`. CI builds no APK; the
F-Droid repository keeps offering what it had (`harken-fdroid` is still
the flake's).

## What a v5 port carries instead

A port of v5 is a port of `rust/ark` as it is now (`spec/README.md`, held
to `spec/vectors` as `spec/RUNNERS.md` says), and it is much less than
this tree (`docs/plan-v5.md` §1.13):

- **one encoding**: a value is bytes under a type — five constructors,
  `Int`, `Bytes`, `List`, `Tuple`, `Sum`, canonical, prefix-free and
  order-preserving, so comparing is `memcmp` and hashing hashes the
  bytes. No `Value` enum, no CBOR, no `Null`, no separate order;
  refinements (`where` clauses) are checked once, by `parse`, at the
  boundary, and are what a form's validation is;
- **one language**: every mutator, query, scope, guard and live
  transition is one typed `Expr` with an effect, checked by one checker
  and run by one interpreter — not five sub-languages each encoded and
  verified on its own;
- **one machine**: the log is a chain of entries each carrying its facts
  and the state after it, and one sans-io `Peer` machine is replica or
  authority by role; a replica applies confirmed entries by their facts
  and runs only its own pending intents. No inbox, no `log_id`, no
  `verify`/`agree`, no separate client, authority and alone code;
- **one view engine**: a query is maintained per operator from the change
  lists every step reports, and partial sync is that same view per
  connection;
- **"volatile" as a flag**: live state is tables marked `Volatile`, a
  second machine of the same kind over them on the same socket — not a
  runtime of transitions, orderings and per-device slots of its own.

The v3 history of this tree: frozen at `4bacc8f` while v4 moved, and
brought to v4 as a client by `docs/plan-phones.md` rounds 2–4.
