# ark-server

An ArkDB sync server for any app's module, on axum 0.7. What `petros-axum`
plus harken's server (`hub.rs`, `persist.rs`, `web.rs`) were, generic: no
app is named here, and an app's server is a builder and its own routes.

```text
axum ── /sync ─────── the socket (binary canonical CBOR, pings) ──┐
     ── /healthz                                                  ├── HubHandle ── the hub's thread:
     ── /auth/*   (ark-auth, when given)                          │     ark::protocol::Server<Relay>
     ── /media    (a directory, range requests)                   │       the Authority, procedures native
     ── /*        (a web build, the build as its validator)       │       live rooms → the app's `Live`
     ── the app's own routes ─────────────────────────────────────┘     the log and the kept rooms, on disk
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
        .data(&data)                      // log.ark-log, live.cbor
        .auth(auth.clone())               // /auth/*, and the authenticator at Hello
        .live(MyDesk::new())              // the app's rooms
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

    // A device the server stands in for (a speaker), in an account's room:
    let conn = hub.stand("alice", "media_player.kitchen", |frame| { /* to the device */ })?;
    hub.say(conn, frame)?;  hub.detach(conn)?;

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

## Live rooms

A room is one account: every connection the server verified as the same
user. The app's `Live` is one value holding every room, told — on the hub's
thread, one call at a time —

- `open(room, kept)` when a room's first peer arrives, with the snapshot it
  kept last time (possibly before a restart);
- `join`, `say(peer, frame)`, `part` — each with a `Post` to `tell(who, ..)`
  a device by its stable name (a login's session, or a standing device's
  name), `tell_conn`, `tell_room`, and `keep()` to write the room's snapshot
  now;
- `snapshot(room)` when asked to keep and when the room empties, then
  `close(room)`.

The snapshots are `live.cbor` beside the log. Frames are opaque bytes; an
ArkDB app makes them canonical CBOR of a `Value`. A repeated `Hello` on one
connection — a client paging a long log — leaves the peer where it is: the
room hears nothing (`tests/live.rs` holds it with 301 entries). A standing
peer keeps a room open like any other; an app that wants a room to close
with its last *client* takes its devices out in `part`.

`Echo` (every frame to the rest of the room) and `Quiet` are provided.

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
| `petros_axum::Hub::<App>::open_live(conn, auth, desk)` + `Router::new().route("/sync", get(petros_axum::sync::<App>))` | `ark_server::builder(domain).data(dir).auth(auth).live(desk).build()?` — `/sync` and `/healthz` included |
| `petros::Live` (`join`, `say`, `part`, `snapshot`, `wake`, `close`; `Post::{tell, tell_room, keep, peers, here}`) | `ark_server::Live` (`open(room, kept)` is `wake` + the room opening; the rest the same, over bytes) |
| `Hub::stand(identity, tx)`, `Hub::say`, `Hub::unstand` | `HubHandle::stand(room, who, callback)`, `say`, `detach` |
| `Hub::local()` + `Hub::exchange` for the scanner | `HubHandle::dial()` → an `ark_client::Peer` transport |
| `hub.server()` under a mutex | `hub.read(|h| ..)` on the hub's thread (`authority()`, `rows(table)`, `rooms()`, `identity(conn)`, `health()`) |
| `web::router(dir)`, `web::build_tag(dir)` | `web::router_with_module(dir, module)`, `web::build_tag(dir, module)` |
| `.nest_service("/media", ServeDir::new(dir))` | `.media(dir)` |
| harken's `persist.rs` (`<scope>.ark-log`) | `persist.rs`: one log, a snapshot `log.ark-log` and the append-only `log.ark-journal` after it, synced before anything it holds is acknowledged |
| `/healthz` per scope | `/healthz`: `ok`, `connections N`, `head N`, `room R peers N` |

## Tested, and not

`cargo test -p ark-server`, all against the demo module over real sockets:
two peers syncing and an offline edit rebased on reconnect; a maintained
view following the log patch by patch; pending intents, the confirmed store
and the server's log across restarts; a peer alone; a frame that is not the
protocol closing that socket and nothing else, and `/healthz`; a peer in the
server's process with no socket; live frames within an account and not
across, dropped while unlinked; a repeated Hello that is paging and not a
departure; a room's snapshot kept when it empties and across a restart; a
standing device hearing and speaking; the ark-auth login on the socket, a
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
