# ark-auth

Who an ArkDB peer is. `petros-auth`, on ArkDB.

The engine asks one question about identity, once per connection: at
`Hello`, what does this token prove? This crate signs people in for a
server — OpenID Connect, or dev auth by name — and mints the token; the
session it stands for is a row of the log (`docs/plan-v5.md` S1), which
`ark-peer`'s hub writes as its authority and judges every token by.

- **server** (natively): the one relying party. `Auth::new(mode, public_url,
  key)` holds the mode (`Mode::Oidc(Provider)` or `Mode::Dev`), the pending
  logins and the one-minute single-use codes; `server::router(auth)` is
  `/auth/login`, `/auth/callback`, `/auth/exchange` as an axum 0.7 `Router`.
- **sessions**: a session is one login on one device, and a row of the
  domain's `sign_in` table (`server::Opened`: id, person, name, email,
  device, issued and expiry times, revoked, roles) that `Auth::on_sign_in`
  hands the hub to write before the token goes out. The token is
  `id.hmac_sha256(key, id)` in hex (`Auth::mint`, `Auth::verify`): the row
  holds no secret, and the server keeps only its key (`server::key_file`,
  made at first start). Who is signed in and signing out are the domain's
  `session.current` and `session.revoke`; the hub closes the connections of
  a session revoked or expired.
- **dev auth**: `Mode::Dev` signs anyone in as the name they give — at once
  when the login URL carries `user=`, through a one-field form otherwise —
  and `name:role,role` as that name holding those roles (`alice:library`,
  read by `server::dev_account`). `Auth::announce()` is what a server prints
  at startup, every time: the mode's sentence (dev auth's names the
  `name:role,role` form) and every role the configuration grants.
- **roles** (`docs/plan-auth.md`): a claim about a person the log does not
  hold, which a guard asks with `has_role` — of the roles the server stamps
  on every entry the login pushes (`docs/plan-guards.md` D1). A login holds
  the roles it was issued with — kept on its session row as `roles`: the
  server's own scanner's by construction, a dev login's from its name —
  those the domain's `role` table grants its person (an admin's
  `role.grant`), and the ones `Auth::with_roles([(role, [account id…])])`
  grants its account from configuration, merged at every judging and never
  written into a row, so a restart with other configuration grants or
  revokes for every login at once. `Account.roles` carries the issued and
  configured ones to the client in the exchange's answer; a client hands
  them to its peer as the `Who` it authors as (`ark_peer::who`), and the
  server takes a connection's roles from its judging whatever the client
  says. A provider's groups claim is not read.
- **OIDC** (the `openidconnect` crate, 4.x, over the same blocking `ureq`
  agent — its `SyncHttpClient` — and never reqwest): authorization code with
  PKCE (S256), as a confidential client; discovery from the issuer and its
  key set; the secret read from a file (`oidc::read_secret`,
  `Provider::discover_with_secret_file`) — a systemd credential, never an
  argument, an environment variable or a store path. The crate checks the ID
  token: signature, issuer, audience, expiry and nonce.
- **client**: natively, `client::login(server, user,
  open)` listens on a loopback port, asks the server's login without
  following the redirect (a dev server answers with the code at once — no
  browser), otherwise opens the system browser with `open` and waits for the
  code; `exchange`, `open_browser`. In a browser,
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
use ark_auth::server::{key_file, Auth, Mode};
use ark_auth::oidc::Provider;

let key = key_file(&data.join("session.key"))?;
let mode = match issuer {
    Some(iss) => Mode::Oidc(Provider::discover_with_secret_file(&iss, &client_id, &secret_file, &["openid", "profile", "email"])?),
    None => Mode::Dev, // only when asked for, and said loudly
};
let auth = Arc::new(
    Auth::new(mode, &public_url, &key)
        .allow_redirect("myapp://")
        // role → the account ids holding it: the provider's `sub`, or a dev name
        .with_roles([("library", vec!["scanner"]), ("admin", vec!["a1b2-sub"])]),
);
// ark-peer: `Server::new(config).auth(auth.clone())` mounts the routes,
// writes each sign-in's row and judges every token by it. After `build`,
// a peer the server runs itself signs in like everyone else:
let scanner_login = auth.issue(&ark_auth::Account {
    id: "library".into(),
    roles: vec!["library".into()], // by construction: kept on its session
    ..Default::default()
}, "the scanner")?;
```

## A client

```rust
// desktop, off the UI thread (it blocks as long as the person takes)
let login = ark_auth::client::login(&server, Some("alice"), ark_auth::client::open_browser)?;
ark_auth::remember::Logins::new("myapp").remember(&server, &login);
// then an ark-peer device, as that person on that device:
peer.login(ark_peer::who(&login), login.token.clone());
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
| `client::{login, exchange, open_browser}` | the same; `whoami`, `logout` are the domain's `session.current`, `session.revoke` |
| `web::{go_sign_in, take_code, exchange, storage}` | the same |
| iced's hand-written `remembered::{recall, remember, forget}` | `remember::Logins::new(app).{recall, remember, forget}` |
| `session::SessionStore::open(petros::open_path(..))` (SQLite, Diesel) | a row of the domain's `sign_in` table, written by the hub |
| `SessionStore::issue(&account)` before `Auth::new` | `Auth::issue(&account, device)` once the hub is built |
| `impl petros::Authenticate for Authenticator` (`authenticate` + `owns`) | `ark_peer::serve::judging(auth, hub)` (a token to an `ark::peer::Who`) |
| `Mode::{Oidc, Dev}`, `Auth::new(..).allow_redirect(..)`, `server::router` | the same (axum 0.7) |
| a secret passed in | `oidc::read_secret(path)`, `Provider::discover_with_secret_file` |

## What the crate does differently from the hand-rolled client

- **The ID token's signature is checked**, against the provider's key set
  (or HS256 with the client secret, where the provider offers it). The key
  set is fetched at discovery and again at every exchange, so a provider
  rotating its keys is followed without a restart. Before, the signature
  was not checked (it came over TLS in the answer to this server's own
  request, which OpenID Connect Core 3.1.3.7 allows).
- **The issuer is compared exactly**: the configured issuer must be what
  the provider's discovery document says, trailing slash and all (before,
  a trailing slash was trimmed on both sides). Auth0's issuer ends in `/`;
  most others do not.
- **The audience is stricter**: an ID token naming another audience beside
  this client is refused (the crate trusts no other audience); before, any
  `aud` containing the client was taken.
- **The nonce is required**, as before: a token without one, or with
  another login's, is refused. `iat` is not checked, as before.
- **The client secret goes to the token endpoint as HTTP Basic**
  (`client_secret_basic`, the spec's default every provider supports);
  before, it was posted in the form body (`client_secret_post`).
- **userinfo** is asked only when the ID token lacks a name or an email, as
  before, and now must name the same subject.
- A provider's error at the token endpoint is reported as its HTTP status
  (the ureq client gives the crate no body for a 4xx).

## Tested, and not

`cargo test -p ark-auth` runs the redirect rules, a code redeemed once for
the row written, the tokens (another key's signature, another id, a changed
signature refused), the key file, roles (a login's own and the configured
ones; dev auth's `name:role,role`), the whole dev login over real HTTP (no
browser, the row and its device, two logins as two sessions, a code refused
the second time and a foreign redirect refused, the dev form), and
`tests/oidc.rs`: a login through a provider served in the test (discovery,
the redirect with PKCE, state, nonce and scopes, the callback's exchange,
the subject, name and email, the row written), and an ID token refused for
another login's nonce, for a signature not made with the secret, and a
state never started. Judging at the hub — revoked, expired, a token naming
no row — is `harken/server/tests/sessions.rs`.

Not verified: a real OpenID Connect provider (the test's is HS256 and a few
routes; RS256 against a provider's key set is the crate's, untested here);
the desktop's browser flow with a person in it (the loopback listener is only
exercised by the dev path, which never opens a browser); and the wasm half in
a browser — it compiles for `wasm32-unknown-unknown` and nobody has run it.
