# ark-server

An ArkDB sync server for any app's module, on axum 0.7. What `petros-axum`
plus harken's server (`hub.rs`, `persist.rs`, `web.rs`) were, generic: no
app is named here, and an app's server is a builder and its own routes.

```text
axum ── /sync ─────── the socket (binary canonical CBOR, pings) ──┐
     ── /healthz                                                  ├── HubHandle ── the hub's thread:
     ── /auth/*   (ark-auth, when given)                          │     ark::protocol::Server
     ── /media    (a directory, range requests)                   │       the Authority, procedures native
     ── /*        (a web build, the build as its validator)       │       the module's live states
     ── the app's own routes ─────────────────────────────────────┘     the log and the kept live state, on disk
```

## An app's server

```rust
#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let data = PathBuf::from(env("APP_DATA").unwrap_or("app-data".into()));
    let sessions = SessionStore::open(data.join("sessions.json")).map_err(anyhow::Error::msg)?;
    let mode = match env("APP_OIDC_ISSUER") {
        Some(iss) => Mode::Oidc(Provider::discover_with_secret_file(&iss, &client_id, &secret_file, &["openid", "profile", "email"]).map_err(anyhow::Error::msg)?),
        None if env("APP_DEV_AUTH").is_some() => Mode::Dev,
        None => anyhow::bail!("no provider and no APP_DEV_AUTH=1"),
    };
    let auth = Arc::new(Auth::new(sessions, mode, &public_url).allow_redirect("myapp://"));

    let app = ark_server::builder(Domain::new(&my_domain::module()))
        .name("myapp")
        .data(&data)                      // log.ark-log, live-states.cbor
        .auth(auth.clone())               // /auth/*, and the authenticator at Hello
        .media("/srv/media")              // /media
        .web(web_dir).web_module("pkg/myapp_bg.wasm")
        .merge(Router::new().route("/api/thing", get(thing)))
        .build()?;
    let hub = app.hub.clone();            // for in-process peers and devices
    let running = app.serve("127.0.0.1:8787").await?;

    // A peer in this process, signed in like everyone else (a scanner):
    let login = auth.issue(&Account { id: "library".into(), ..Default::default() }).map_err(anyhow::Error::msg)?;
    let mut scanner = ark_client::Peer::open_path(domain, data.join("scanner"),
        Options::server(login.user.id, login.session, Some(login.token)))?;
    scanner.connect_with("local", hub.dial());   // no socket; pump it on a thread

    // A device the server stands in for (a speaker), in live instances:
    let conn = hub.participate(who, device, instances, |frame| { /* applied, refused, live_snapshot */ })?;
    hub.call(conn, call)?;  hub.detach(conn)?;

    tokio::signal::ctrl_c().await?;
    running.stop().await;
    Ok(())
}
```

`build()` refuses a server told nothing about sign-in: `.auth(..)` for
ark-auth (which also hands the engine `Auth::owns`, so an entry authored
offline under an older login of the same person is accepted after they sign
in again), `.trusting()` for the engine's dev auth (a token is a name, every
session is `"dev"`, no routes), or `.authenticate(f, announcement)`. Whatever
it is is printed at startup, every time.

## Live state

The module's live states (`docs/plan-live.md`, spec §14) are served on
`/sync` beside the log: the hub sequences each instance's calls at its own
clock, relays what each instance took to the devices in it, and keeps each
instance's ordered part in `live-states.cbor` beside the log, restored at
start through the state's `on_restore`. Nothing of the app's is asked: the
module says it all.

A device the server stands in for — a speaker a bridge reconciles — is a
**participant** (`HubHandle::participate`): in the instances it is given,
as the identity it is given, hearing their frames, calling with
`HubHandle::call`, leaving with `detach`. It is not a replica, and hears
no log.

It replaced opaque rooms — a room per account, frames the server never
looked inside, an app's `Live` told who arrived, spoke and left, its
snapshots in `live.cbor` — which nothing uses since harken's listening
session became a live state (`docs/plan-live.md` phase 2). A `say` from a
peer of an older revision is read and dropped rather than cutting it off,
and nothing is sent as `heard`. A `live.cbor` an older server left is not
read, and can be deleted.

## Serving a web build

`web::router` / `.web(dir)`: every file under the directory carries the
build as its `ETag` (the store hash of a `/nix/store` path; otherwise the
named module's mtime and length, or every file's), `Cache-Control:
no-cache`, and no `Last-Modified`; `If-Modified-Since` is stripped from the
request so a browser holding the store's 1970 date gets `200`, and a miss
falls back to `index.html`. `web.rs` explains why each is needed — it is
harken's `server/src/web.rs` made generic.

## From petros-axum and harken's server

| before | ark-server |
|---|---|
| `petros_axum::Hub::<App>::open_live(conn, auth, desk)` + `Router::new().route("/sync", get(petros_axum::sync::<App>))` | `ark_server::builder(domain).data(dir).auth(auth).build()?` — `/sync` and `/healthz` included |
| `petros::Live` (`join`, `say`, `part`, `snapshot`, `wake`, `close`; `Post::{tell, tell_room, keep, peers, here}`) | a live state in the module (`docs/plan-live.md`): transitions and the `on_join`, `on_leave` and `on_restore` hooks, run by the server |
| `Hub::stand(identity, tx)`, `Hub::say`, `Hub::unstand` | `HubHandle::participate(who, device, instances, callback)`, `call`, `detach` |
| `Hub::local()` + `Hub::exchange` for the scanner | `HubHandle::dial()` → an `ark_client::Peer` transport |
| `hub.server()` under a mutex | `hub.read(|h| ..)` on the hub's thread (`authority()`, `rows(table)`, `rooms()`, `identity(conn)`, `health()`) |
| `web::router(dir)`, `web::build_tag(dir)` | `web::router_with_module(dir, module)`, `web::build_tag(dir, module)` |
| `.nest_service("/media", ServeDir::new(dir))` | `.media(dir)` |
| harken's `persist.rs` (`<scope>.ark-log`) | `persist.rs`: one log, a snapshot `log.ark-log` and the append-only `log.ark-journal` after it, synced before anything it holds is acknowledged |
| `/healthz` per scope | `/healthz`: `ok`, `connections N`, `head N`, `room R peers N` (an account and its open replica connections) |

`/healthz` under `Accept: application/json` is one object
(`ark_server::health_json`): head, horizon, log id, modules run,
connections, rooms, and every session with its cursor, last-heard time and
open connections. `arkc backup DIR OUT` copies a running server's data
directory consistently (the snapshot, then the journal up to its length at
that moment, then the rest), and `arkc restore BACKUP DIR` puts it back
into an empty directory as a log the server names afresh. `arkc verify-log
DIR [M]` reads one: head, horizon, log id, modules run, and with the module
the state hash at the head.

## Tested, and not

`cargo test -p ark-server`, all against the demo module over real sockets:
two peers syncing and an offline edit rebased on reconnect; a maintained
view following the log patch by patch; pending intents, the confirmed store
and the server's log across restarts; a peer alone; a frame that is not the
protocol closing that socket and nothing else, and `/healthz`; a peer in the
server's process with no socket; live state across a fleet of devices
(`tests/live_fleet.rs`); the ark-auth login on the socket, a
revoked token denied with the link stopped and the pending entry kept,
somebody else's token refused, an entry from an earlier login of the same
person accepted after signing in again; a server with no sign-in refusing to
start;
the web validator exchanges (a stale `If-Modified-Since` for 1970 answered
200 with the files' mtimes set to that very second, `If-None-Match`
answered 304, a rebuild a new tag). Each was falsified once by breaking what
it holds.

Not verified: a deployment behind a real TLS proxy, an OpenID Connect
provider, `/media` range requests from a real player.
