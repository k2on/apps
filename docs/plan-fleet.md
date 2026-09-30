# The fleet: real peers, a real server, and the network taken away

`ark::sim` already runs a seeded fleet in one process — replicas, an
authority, a network that reorders, duplicates and drops — and every
runtime is held to its `rebase/` vectors. What it cannot see is everything
outside the engine: the WebSocket transport and its keepalive, a server
process that is killed and restarted from its log on disk, a peer killed
between writing its intent and pumping, a connection cut in the middle of a
page, the sign-in a peer does before its first frame, the scanner authoring
into a log while a client is away. Those are where the remaining edge cases
are, and this is the plan for a harness that reaches them: the real
binaries, a network that can be taken away in every way a laptop lid and a
bad hotel wifi take it away, and one invariant checked after every scenario
— **every replica's state hash equals the server's once the network is
back**, and every accepted intent appears in the log exactly once.

Two layers, because they answer different questions and run in different
places:

1. **A process fleet, in `cargo test`.** The real `harken-server` binary and
   real headless peers as child processes on loopback, joined through a
   small TCP proxy the test controls (pause, black-hole, cut, delay,
   replay). Runs anywhere `cargo test` runs — this container, `nix flake
   check`, CI — with no virtualisation. This is where the scenarios live and
   where the seeded fuzz runs for as long as one cares to let it.
2. **A NixOS VM test.** The same peer binary on separate machines under the
   real `services.harken` module: `ip link` down and up, `systemctl restart
   harken`, the scanner over a bound media directory, real DNS between
   hosts. It needs KVM (this container has none; under TCG it runs, slowly),
   so it is a package to build on a machine that has it rather than a check
   every `nix flake check` pays for.

## 1. The headless peer: `harken-peer`

A binary in the `harken/server` package (`harken/server/src/bin/
harken-peer.rs`), because that package already links the domain, `ark-client`
and `ark-auth` (the scanner is a peer), and because `CARGO_BIN_EXE_*` only
reaches binaries of the crate whose tests run. It is a peer with no screen —
also useful on its own for scripting a server from a shell.

```
harken-peer --dir DIR --server URL [--user NAME] [--alone]
```

It opens the replica in `DIR` (`Peer::open_path`), signs in the way the
desktop does in dev auth (`ark_auth::client::login(server, Some(name),
|_| {})` — a dev server answers the first request with the code and opens no
browser; the login is remembered under `DIR` so a restart reuses it),
connects to `ws://…/sync`, pumps on a timer (the desktop's 50 ms), and reads
**commands as JSON lines on stdin, answering one JSON line each on stdout**,
so a test drives it without a socket of its own:

| command | answer |
|---|---|
| `{"cmd":"mutate","name":"add_to_playlist","args":{…}}` | `{"ok":true,"id":"…"}` or `{"ok":false,"why":"…"}` (a refusal at authoring) |
| `{"cmd":"status"}` | `{"cursor":N,"pending":K,"linked":bool,"denied":null\|"why","user":"…","epoch":N}` |
| `{"cmd":"hash"}` | `{"cursor":N,"hash":"hex"}` — `ark::hash::state_hash` of the **confirmed** store; and `"view":"hex"` of the optimistic one |
| `{"cmd":"wait","cursor":N,"timeout_ms":T}` | when the confirmed cursor reaches `N`, or `{"ok":false}` at the timeout |
| `{"cmd":"settle","timeout_ms":T}` | when `pending == 0`, linked, and the cursor has not moved for two pumps |
| `{"cmd":"query","name":"playlists","args":{}}` | `{"rows":[…]}` as JSON (the vectors' dialect for ids and ints) |
| `{"cmd":"rejections"}` | what `take_rejections` returned since last asked |
| `{"cmd":"disconnect"}` / `{"cmd":"reconnect"}` | the peer's own `disconnect`/`reconnect` (the *engine* offline, distinct from the network being cut) |
| `{"cmd":"sign_in","user":"…"}` / `{"cmd":"sign_out"}` | the late sign-in path (`Peer::sign_in`) |
| `{"cmd":"persist"}` | force a pump-and-persist now (what a test does before killing the process) |
| `{"cmd":"quit"}` | exit 0 after a persist |

Ids, arguments and rows cross as the vectors' JSON dialect (`$id`, `$int`,
`$bytes`), which `rust/ark/src/bin/vectors/json.rs` already defines; move
that dialect into `ark` proper (`ark::json`, encode and decode) so the peer,
the vectors and the tests share one copy. Errors go to stderr, one line
each, prefixed `harken-peer:`, so a test can attach them to a failure.

## 2. The process fleet: `harken/server/tests/fleet.rs`

A `Fleet` in `tests/support/fleet.rs`:

- **`Server`**: spawns `CARGO_BIN_EXE_harken-server` with `HARKEN_DEV_AUTH=1`,
  `HARKEN_DATA=<tempdir>`, `HARKEN_MEDIA=<tempdir>/media` (so the scanner
  runs), on a free loopback port; waits for `/healthz`; `kill9()`,
  `restart()` (same data directory), `stop()` (SIGTERM, for a clean
  shutdown, which should also converge). Its stderr is captured.
- **`Proxy`**: one listening port per peer, forwarding to the server. Per
  peer, from the test thread: `pass()`, `blackhole()` (accept and read, send
  nothing, never close — the lid-closed case the keepalive exists for),
  `cut()` (close both sides now), `pause(dur)` (hold bytes, then release),
  `cut_after(bytes)` (the mid-page case), `delay(dur)`, and `replay_last()`
  (send the last client frame again — a duplicate the server must dedupe by
  entry id). Byte-level, protocol-blind, a few hundred lines.
- **`PeerProc`**: spawns `CARGO_BIN_EXE_harken-peer` pointed at its proxy
  port, with typed helpers over the JSON lines; `kill9()`, `restart()` (same
  dir), `settle()`, `hash()`. A killed peer's directory survives; that is the
  point.
- **`converged(&[peers], &server)`**: settles every peer, reads every hash,
  and reads the server's log (`HARKEN_DATA/log.ark-log`, decodable with
  `ark_server::persist`) to assert: every peer's confirmed hash equals
  `state_hash` of the log's replay; every peer's cursor equals the log's
  head; every intent id appears once in the log; and the union of what the
  peers report accepted is exactly the log's ids. Prints the timings (time to
  converge per scenario), because this harness is also where sync speed is
  measured against a real socket.

Scenarios, each a `#[test]` (the fuzz one `#[ignore]`d for long runs and
run short by default), each named for the case it exists to find and each
**falsified once** by breaking the property it holds (a fixed seed, a
removed dedupe, a skipped persist) with the doc comment saying how:

1. **Three peers online**, interleaved `create_playlist`/`add_to_playlist`/
   `remove_from_playlist`/`add_all_to_playlist` — converge, and the
   playlists' `pos` order is the log's order.
2. **One peer black-holed**, makes twenty intents while the others make
   twenty; released — converge; its adds land after theirs (the rebase,
   visible: `pos` of its items is greater).
3. **Server killed with `-9` mid-stream** while peers are pushing;
   restarted from `log.ark-log`; peers reconnect on their own — converge,
   no intent lost, none twice (the peers re-push pending; the server dedupes
   by id). Also `stop()` cleanly and restart. And the log file after a kill:
   is it whole? `persist.rs` rewrites it whole per batch — check it writes
   to a temporary name and renames, and if it does not, that is a finding
   (a torn log on restart is the worst outcome this harness can catch).
4. **Peer killed with `-9`** right after `mutate` returned and before its
   next pump, restarted — the intent is in `pending` on disk (the durability
   rule), is pushed, converges. And killed right after the server's ack but
   before the peer persisted the cursor — on restart it re-pushes; the
   server answers `Duplicate`; converge.
5. **Cut mid-page during initial sync**: a fresh peer joining a log of 1,500
   entries (seeded by another peer first), `cut_after(N bytes)` twice at
   different points — it resumes at its cursor, never applies a page twice,
   converges; time to first full sync is printed.
6. **Sign in later**: a peer started with no `--user`, authoring as nobody
   while the network is black-holed; then `sign_in alice`; released —
   everything it made is alice's on every peer (`user_id` on the rows), and
   the server accepted it under alice's login.
7. **Same playlist name on two black-holed peers** (`Favorites`) — one is
   `Favorites (1)` everywhere, and which one is decided by log order, the
   same on every peer.
8. **Turned away**: revoke a peer's session at the server (the dev auth's
   session store — `ark_auth::session::SessionStore`; add a way to revoke a
   session, e.g. a `DELETE /auth/session/<id>` in dev mode, or drive the
   `logout` endpoint with that peer's token) while it has pending intents —
   it keeps its store and its pending, reports `denied`, stops reconnecting;
   `sign_in alice` again as the same person — pending is offered under the
   new login (`with_owns`) and converges.
9. **The scanner and an absent peer**: files dropped into `media/music`
   while one peer is black-holed; the server's scanner authors them; the
   peer returns and sees the songs; the files copied again under new names
   are not new songs (idempotency by `file`).
10. **Keepalive**: a peer black-holed with no FIN — the server closes it
    after `Keepalive::missed` pings (shorten the keepalive for the test via
    an environment variable if there is none — say so), the peer notices,
    and on release reconnects and converges; and the live room forgot the
    device meanwhile (`status` of another peer's listening view, if cheap to
    read; otherwise the server's log line).
11. **Replayed frames and reordering**: `replay_last()` on a push and on a
    `Hello` — the server dedupes the push and treats the repeated `Hello` as
    paging, not a departure (the bug `docs/…` records).
12. **The seeded fuzz**: 3–5 peers, a seeded schedule of {mutate, blackhole,
    release, cut, pause, kill peer, restart peer, kill server, restart
    server, sign in later} for N steps, then release everything and
    `converged`. Run 20 steps by default and 2,000 under `FLEET_LONG=1`.
    Every failure prints the seed and the schedule.

What the fleet measures, printed by each scenario and gathered in
`docs/plan-fleet.md`'s closing section when this lands: time for a fresh
peer to sync 1,500 entries over a real socket, time to converge after a
black-hole of twenty intents, server restart time to first re-ack, and the
size of `log.ark-log` against the number of entries.

## 3. The NixOS VM test: `packages.fleet-vm`

`harken/server/nix/fleet-vm.nix`, a `nixosTest` (`pkgs.testers.runNixOSTest`)
with three machines: `server` (`services.harken` with `devAuth`, `address =
"0.0.0.0"`, the media directory seeded with a few files at build time),
`alice` and `bob` (each with `harken-server` in its path for `harken-peer`,
a state directory, and a small systemd service that runs `harken-peer` with
its stdin on a FIFO so the test script can `echo` commands into it and read
its stdout log — or simpler, the test script runs `harken-peer` under
`machine.succeed` with a heredoc per command batch; pick whichever reads
better, and say why). The scenario is the VM-only half of the list above:

- both peers sign in against `server` by hostname (real DNS), converge;
- `alice.fail("ip link set eth1 down")`… `alice.succeed("ip link set eth1
  down")` (the test network is `eth1`), both mutate, `systemctl restart
  harken` on the server meanwhile, `ip link set eth1 up` — converge;
- `server.crash()` and `server.start()` — the service comes back from
  `/var/lib/private/harken`, peers converge;
- files copied into `/srv/media/music` on the server — the scanner authors
  them, both peers see them.

Exposed as `packages.<system>.fleet-vm` (not a check): `nix build
.#fleet-vm` on a machine with `/dev/kvm`. Document in `harken/README.md`
that it needs KVM and roughly how long it takes. From this container it can
only be run under TCG; try it once with a long timeout and report whether it
completed or how far it got — "not verified" is an acceptable answer for
this layer, the process fleet is the one that must pass here.

## 4. What is likely to be found, so the harness is pointed at it

Named here so that a finding is recognised rather than debugged from
scratch:

- `persist.rs` rewrites the whole log per batch, without (as far as reading
  goes) a write-then-rename — a kill during the write is a torn log the
  server refuses to serve on restart. Also O(log) per append: a performance
  item for the other pass.
- `Log::entries_after` clones every retained entry after the cursor before
  taking a page (`rust/ark/src/log.rs`), quadratic for a fresh peer over a
  long log — visible in scenario 5's timing.
- A peer killed between the server's ack and its own persist re-pushes an
  entry the server already has: `Duplicate` must reach `ack` and the cursor
  must still advance past it (scenario 4b).
- A repeated `Hello` on one connection is paging, not a departure (scenario
  11) — fixed once already; the harness keeps it fixed.
- The keepalive is the transport's and the engine has no clock: scenario 10
  is the only place the whole loop is exercised.
- The dev-auth session store may have no revocation path (scenario 8); if
  it has none, adding one is in scope and small.

## 5. Rules

The implementation agent owns `harken/server/**` (the peer binary, the
tests, `nix/fleet-vm.nix`), `rust/ark/src/json.rs` (the dialect moved from
`rust/ark/src/bin/vectors/json.rs`, with that file becoming a re-export),
`rust/ark-auth/**` only for session revocation if scenario 8 needs it,
`flake.nix` for `packages.fleet-vm` and the `harken-server` package carrying
the second binary, `harken/README.md`, and this document's closing section.
Anything found in `rust/ark` or `rust/ark-client` (a torn log, a lost
duplicate ack, a keepalive bug) is **reported with a failing test and a
proposed fix, not fixed in place** — those fixes are decided by the
coordinator and handed out separately, so that a scenario stays a witness
rather than becoming a patch. Every scenario is falsified once. The
commit rules of `docs/plan-v4.md` Part 2 apply.

## Measured

What landed, and what it found. The fleet is `harken/server/tests/fleet.rs`
over `tests/support/{fleet,proxy}.rs`; the peer is
`harken/server/src/bin/harken-peer.rs`; the machines are
`harken/server/nix/fleet-vm.nix`. Numbers are from this container (four
cores, a debug build, nothing else running unless said), printed by
`cargo test -p harken-server --test fleet -- --nocapture`.

| what | took |
|---|---|
| the whole default fleet (13 scenarios, the fuzz at 20 steps; before 3b ran) | 17 s |
| …with 3b, in `cargo test --workspace` beside everything else, after R3 and R4 | 31–32 s |
| a fresh peer syncs 1,500 entries over a real socket (370 KB from the server) | 3.9 s; 2.2 s after R4 |
| …the same, its connection cut twice mid-page | 4.1 s; 3.1–3.2 s after R4 |
| converge after a black hole of twenty intents | 0.29 s |
| three peers online, 25 interleaved playlist moves, converge | 0.29 s |
| the server restarted: to listening | 52–82 ms |
| …to the first re-ack | 0.19–0.40 s |
| …to converged | 0.43–0.76 s |
| a black-holed socket closed by the server (keepalive 300 ms × 3) | 1.03 s |
| …and noticed by the peer on its own | 1.03 s |
| converge after the keepalive closed a black hole | 0.29 s |
| seed 1,500 entries through one peer (`FLEET_LONG=1`, before R3) | 51.5 s |
| the fuzz, 2,000 steps, 3–5 peers, whole run (with the §4 fix below) | 16–38 s |

The log on disk is about 348 bytes an entry for playlists (46 entries,
16 KB; 345 as snapshot and journal after R3) and 521 for the mixed 1,500
of scenario 5 (500 songs, 10 playlists, 990 adds: 782 KB, seeded as a
snapshot).

**Found, and fixed as R3.** `a_server_killed_mid_stream_loses_nothing`
(3b) landed as `#[ignore = "witness: …"]`. `Hub::after` delivered what the
machine queued — the `Ack` to the author, the `Batch` to everyone else —
and only then wrote `log.ark-log`. A `kill -9` between the two left peers
confirmed past the file: over a 1,500-entry log the first of eight rounds
failed, three runs of three —

    round 0: a confirmed up to 1501 and log.ark-log holds 1500: the server said it before it wrote it

— and the restarted server gave those sequences to other entries. Every
cursor then agreed with the head, so nothing retried; the state hashes
differed for ever, and an intent whose ack was taken was stuck pending (its
re-ack named a sequence the peer believed it was past). The long fuzz found
the same thing on its own (`FLEET_LONG=1 FLEET_SEED=1790774728680826455`:
one peer at the head with a different hash and nine pending), and moving
`self.persist()` before the sending loop made both pass. R3 (`3197e8a`,
`a486891`, `19c8bf6`) is the fix as landed: the log is a snapshot
`log.ark-log` and an append-only `log.ark-journal`, synced before anything
it holds is sent. 3b is a plain test now and passes three runs of three;
that seed and seed 99 converge at 2,000 steps. One proposal the witness
argued for is still open: a `Hello` whose `since` is past the log's head
is a peer from a log this server no longer has, and could be answered (a
snapshot, or a denial that says so) rather than served nothing — 3a's
falsification, an emptied data directory, is that case, and today it
leaves every pending intent unacked for ever.

**Looked for, and not found.** The log file was not torn by a kill:
`persist.rs` already wrote a temporary name and renamed it into place (§4
guessed otherwise), and every kill in 3a, 3b and the fuzz read back whole
— before R3 and after it.
A `Duplicate` after a peer's restart does advance its cursor (4b). A
repeated `Hello` on one connection is paging, not a departure (11). The
keepalive closes a black-holed socket at both ends (10). Revoking a session
needed nothing new in `ark-auth`: `/auth/logout` with the peer's token
does it (8).

**Performance, for the other pass.** Before R3 every push rewrote the
whole log, so seeding 1,500 entries through one peer took 51.5 s, about
34 ms an entry at the end (not measured again since the journal); scenario 5 seeds the log before the server starts
instead, and pushes through a peer only under `FLEET_LONG=1`.
`Log::entries_after` cloning the remainder per page was not isolated: a
fresh sync of 1,500 is six pages and 3.9 s, and nothing here says how much
of that is the clone. In the domain, `create_playlist` reads every other
playlist of its owner to name the new one, with a fold whose step filters
the whole list again: 1,500 playlists for one owner did not finish seeding
in twelve minutes, so the witness seeds songs.

**Corrections to this plan.** Files "copied again under new names" are
new songs — a song is its path, in `add_song`; scenario 9 asserts that
rewriting the same names is not, and that new names are. Scenario 8 needed
the socket cut after the revocation: the token is asked at `Hello` and not
again, so a revoked session's open connection goes on until it drops.

**The harness's own bugs, each found by a scenario.** `harken-peer`
pumped only when stdin was quiet, so a driver polling `status` every ten
milliseconds starved it (it keeps its own clock now). `converged` forgave
any refusal and so passed scenario 8's falsification (a refusal now fails
every scenario but the fuzz). The second mid-page cut was armed after the
first fired, which a peer quick to redial outran under load (cuts queue).
Rejections are written to `DIR/rejections.jsonl` as they arrive, because
the engine keeps a verdict only in memory and a peer killed before it was
asked took the verdict with it.

**`fleet-vm`, under TCG.** The machines ran here with no `/dev/kvm`:
the test declares the `kvm` feature, so `nix build .#fleet-vm --option
system-features "nixos-test benchmark big-parallel kvm"` lets qemu fall
back to emulation. All four subtests passed — sign-in by hostname and the
scanner's two songs (12 s), both interfaces down with the service
restarted underneath (15 s), a crash and a boot from
`/var/lib/private/harken` (123 s, nearly all of it the boot), and files
copied into `/srv/media/music` (8 s) — in a 328 s test script, eleven
minutes from start to finish. The crash did not find the missing `fsync`:
the log had been written back before it. Its first run found a harness
mistake instead: `settle` is "nothing pending, the cursor still for two
pumps", which a peer under emulation satisfied before its first batch
arrived, so the machines wait for the head `/healthz` reports.

**In the sandbox.** `nix flake check` at `a3ef488` passed: its `rust`
check runs the fleet in the build sandbox on loopback, in release — 13
passed and the witness ignored, in 8 s. After R3, `cargo test --workspace`
here ran it twice beside everything else, 14 passed each time.
