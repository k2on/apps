# ark-auth

Who an ArkDB peer is. `petros-auth`, on ArkDB.

The engine asks one question about identity, once per connection: at
`Hello`, what does this token prove (`ark::protocol::Authenticate`)? This
crate answers it for a server that signs people in with OpenID Connect, and
gives the clients the other half — how to get a token to send.

- **server** (feature `server`): the one relying party. `Auth` holds the
  mode (`Mode::Oidc(Provider)` or `Mode::Dev`), the session store, the
  pending logins and the one-minute single-use codes; `server::router(auth)`
  is `/auth/login`, `/auth/callback`, `/auth/exchange`, `/auth/me`,
  `/auth/logout` as an axum 0.7 `Router`; `auth.authenticator()` is what the
  sync server asks at every `Hello`.
- **sessions**: a session is one login on one device. The store keeps a
  SHA-256 of each token, the account, issue and expiry times, and whether it
  was revoked — never the token — in a JSON file written whole (temporary
  name, then rename) on every issue and revoke, or in memory. A session is
  never forgotten: an entry authored under an expired or revoked one is still
  its owner's (`Auth::owns`).
- **dev auth**: `Mode::Dev` signs anyone in as the name they give — at once
  when the login URL carries `user=`, through a one-field form otherwise —
  and `name:role,role` as that name holding those roles (`alice:library`,
  read as the engine's own dev auth reads a token,
  `ark::protocol::dev_identity`). `Auth::announce()` is what a server prints
  at startup, every time: the mode's sentence (dev auth's names the
  `name:role,role` form) and every role the configuration grants.
- **roles** (`docs/plan-auth.md`): a claim about a person the log does not
  hold, which the guards of `docs/plan-guards.md` G2 are to ask (the row
  rules that asked it are deleted, G1). A login holds
  the roles it was issued with — kept on its session as `roles`, absent when
  none: the server's own scanner's by construction, a dev login's from its
  name — and the ones `Auth::with_roles([(role, [account id…])])` grants
  its account from configuration, merged at every ask and never written
  into a session, so a restart with other configuration grants or revokes
  for every login at once. `Account.roles` carries them to the client in
  the exchange's and `/auth/me`'s answers (absent when none, so a login
  remembered before roles reads as one holding none); a client hands them
  to its peer (`ark_client::Peer::set_roles`), whose `Ctx` carries them,
  and the server takes a connection's roles from what it asks here
  whatever the client says.
  `auth.authenticator()` answers `Identity { user, session, roles }`. A
  provider's groups claim is not read.
- **OIDC**: authorization code with PKCE, as a confidential client;
  discovery from the issuer; the secret read from a file
  (`oidc::read_secret`, `Provider::discover_with_secret_file`) — a systemd
  credential, never an argument, an environment variable or a store path.
  The ID token's signature is not checked, for the reason `oidc.rs` gives
  (it arrives over TLS in the direct answer to this server's own request);
  issuer, audience, expiry and nonce are.
- **client** (feature `client`): natively, `client::login(server, user,
  open)` listens on a loopback port, asks the server's login without
  following the redirect (a dev server answers with the code at once — no
  browser), otherwise opens the system browser with `open` and waits for the
  code; `exchange`, `whoami`, `logout`, `open_browser`. In a browser,
  `web::go_sign_in` sends the page to sign in and back to itself,
  `web::take_code` takes `?code=` out of the address bar, and
  `web::exchange` trades it (fetch). `remember::Logins::new("app")` keeps a
  login per server — `~/.config/app/logins.json`, or `localStorage` under
  `app.login.<server>`.
- **everywhere**: `Login { token, session, user: Account, expires_ms }`,
  `login_url`, `socket_url` (the sync socket from the server's base URL,
  scheme to match), `query_value`, `percent_encode`/`decode`, `with_code`.

## A server

```rust
use std::sync::Arc;
use ark_auth::server::{Auth, Mode};
use ark_auth::session::SessionStore;
use ark_auth::oidc::Provider;

let sessions = SessionStore::open(data.join("sessions.json"))?;
let mode = match issuer {
    Some(iss) => Mode::Oidc(Provider::discover_with_secret_file(&iss, &client_id, &secret_file, &["openid", "profile", "email"])?),
    None => Mode::Dev, // only when asked for, and said loudly
};
let auth = Arc::new(
    Auth::new(sessions, mode, &public_url)
        .allow_redirect("myapp://")
        // role → the account ids holding it: the provider's `sub`, or a dev name
        .with_roles([("library", vec!["scanner"]), ("admin", vec!["a1b2-sub"])]),
);
// ark-server: `.auth(auth.clone())` mounts the routes and asks
// `auth.authenticator()` at every Hello.
// A peer the server runs itself signs in like everyone else:
let scanner_login = auth.issue(&ark_auth::Account {
    id: "library".into(),
    roles: vec!["library".into()], // by construction: kept on its session
    ..Default::default()
})?;
```

## A client

```rust
// desktop, off the UI thread (it blocks as long as the person takes)
let login = ark_auth::client::login(&server, Some("alice"), ark_auth::client::open_browser)?;
ark_auth::remember::Logins::new("myapp").remember(&server, &login);
// then an ark-client peer, as that person on that device:
let opts = ark_client::Options::server(login.user.id.clone(), login.session.clone(), Some(login.token.clone()));
peer.connect(&ark_auth::socket_url(&server));

// browser
if let Some(code) = ark_auth::web::take_code() {
    let login = ark_auth::web::exchange(&server, &code).await?;
} else {
    ark_auth::web::go_sign_in(&server, None); // the page goes, and comes back with ?code=
}
```

## From petros-auth

| petros-auth | ark-auth |
|---|---|
| `Login`, `Account`, `socket_url`, `query_value`, `login_url` | the same |
| `client::{login, exchange, whoami, logout, open_browser}` | the same |
| `web::{go_sign_in, take_code, exchange, whoami, storage}` | the same |
| iced's hand-written `remembered::{recall, remember, forget}` | `remember::Logins::new(app).{recall, remember, forget}` |
| `session::SessionStore::open(petros::open_path(..))` (SQLite, Diesel) | `SessionStore::open(path)` (a JSON file) / `SessionStore::memory()` |
| `SessionStore::issue(&account)` before `Auth::new` | `Auth::issue(&account)` after it, or the store's `issue` |
| `impl petros::Authenticate for Authenticator` (`authenticate` + `owns`) | `auth.authenticator()` → `ark::protocol::Authenticate`; `auth.owns(user, session)` / `auth.owns_fn()` → `Server::with_owns` (ark-server's `.auth` wires it) |
| `Mode::{Oidc, Dev}`, `Auth::new(..).allow_redirect(..)`, `server::router` | the same (axum 0.7) |
| a secret passed in | `oidc::read_secret(path)`, `Provider::discover_with_secret_file` |

## Tested, and not

`cargo test -p ark-auth` runs the session store (expiry, revocation, a file
across a reopen holding no token), the redirect rules, a code redeemed once,
the authenticator, roles (a login's own and the configured ones merged in
the exchange and at `Hello`, the configuration moved under the same session
store, dev auth's `name:role,role`), and the whole dev login over real HTTP: login with no
browser, `whoami`, two logins as two sessions, logout, a code refused the
second time and refused a foreign redirect, the dev form. ark-server's
`tests/auth.rs` takes the token onto the sync socket.

Not verified: an OpenID Connect provider — none is reachable from a test
machine, so `Mode::Oidc` (discovery, the callback, the token exchange) is the
petros-auth code ported, unit-tested on its claims checks and nothing more;
the desktop's browser flow with a person in it (the loopback listener is only
exercised by the dev path, which never opens a browser); and the wasm half in
a browser — it compiles for `wasm32-unknown-unknown` and nobody has run it.
