# ark-auth

Who an ArkDB peer is. `petros-auth`, on ArkDB.

The engine asks one question about identity, once per connection: at
`Hello`, what does this token prove (`server::Authenticate`, the `ark::peer::Who`
it proves, as `ark-server` asks it)? This
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
  read by `server::dev_account`). `Auth::announce()` is what a server prints
  at startup, every time: the mode's sentence (dev auth's names the
  `name:role,role` form) and every role the configuration grants.
- **roles** (`docs/plan-auth.md`): a claim about a person the log does not
  hold, which a guard asks with `has_role` — of the roles the server stamps
  on every entry the login pushes (`docs/plan-guards.md` D1). A login holds
  the roles it was issued with — kept on its session as `roles`, absent when
  none: the server's own scanner's by construction, a dev login's from its
  name — and the ones `Auth::with_roles([(role, [account id…])])` grants
  its account from configuration, merged at every ask and never written
  into a session, so a restart with other configuration grants or revokes
  for every login at once. `Account.roles` carries them to the client in
  the exchange's and `/auth/me`'s answers (absent when none, so a login
  remembered before roles reads as one holding none); a client hands them
  to its client as the `Who` it authors as (`ark_client::auth::who`),
  and the server takes a connection's roles from what it asks here
  whatever the client says.
  `auth.authenticator()` answers `Who { actor, session, roles }`. A
  provider's groups claim is not read.
- **OIDC** (the `openidconnect` crate, 4.x, over the same blocking `ureq`
  agent — its `SyncHttpClient` — and never reqwest): authorization code with
  PKCE (S256), as a confidential client; discovery from the issuer and its
  key set; the secret read from a file (`oidc::read_secret`,
  `Provider::discover_with_secret_file`) — a systemd credential, never an
  argument, an environment variable or a store path. The crate checks the ID
  token: signature, issuer, audience, expiry and nonce.
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
| `impl petros::Authenticate for Authenticator` (`authenticate` + `owns`) | `auth.authenticator()` → `server::Authenticate` (a token to an `ark::peer::Who`); `auth.owns(user, session)` |
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

`cargo test -p ark-auth` runs the session store (expiry, revocation, a file
across a reopen holding no token), the redirect rules, a code redeemed once,
the authenticator, roles (a login's own and the configured ones merged in
the exchange and at `Hello`, the configuration moved under the same session
store, dev auth's `name:role,role`), the whole dev login over real HTTP
(login with no browser, `whoami`, two logins as two sessions, logout, a code
refused the second time and refused a foreign redirect, the dev form), and
`tests/oidc.rs`: a login through a provider served in the test (discovery,
the redirect with PKCE, state, nonce and scopes, the callback's exchange,
the subject, name and email), and an ID token refused for another login's
nonce, for a signature not made with the secret, and a state never started.
ark-server's `tests/auth.rs` takes the token onto the sync socket. The
hand-rolled parsing's tests (base64url, a JWT's payload, the claims checks)
went with it: the crate owns that.

Not verified: a real OpenID Connect provider (the test's is HS256 and a few
routes; RS256 against a provider's key set is the crate's, untested here);
the desktop's browser flow with a person in it (the loopback listener is only
exercised by the dev path, which never opens a browser); and the wasm half in
a browser — it compiles for `wasm32-unknown-unknown` and nobody has run it.
