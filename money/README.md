# money, on ArkDB

A household's budget: the accounts, the month's categories, what each
was given and spent, and the bank's transactions through Plaid. All of it
runs on ArkDB at spec v5, as harken does: one domain in plain Rust, every
figure a view kept up to date by the rows a change touched.

```
domain/    the domain, in plain Rust (spec/AUTHORING.md): src/schema.rs,
           types.rs, dates.rs; accounts.rs, categories.rs, budget.rs,
           transactions.rs; plaid.rs, the bank import; seed.rs, the
           demo's household (the `seed` feature)
server/    money-server: the hub, sign-in, the browser client, and the
           bank importer (src/importer.rs, Plaid over src/plaid.rs);
           nix/module.nix is services.money
iced/      the client, desktop and browser, on arkui and ark-peer: signed
           in to a server, or (--features demo) a seeded household on a
           peer that is its own authority
ffi/       the library the phones load (ffi/README.md): ark-ffi with the
           domain linked in, typed views, the demo, a server in the process
ios/       the iPhone app (ios/README.md): SwiftUI on ffi/, with the demo
android/   the Android app (android/README.md): Material You on ffi/,
           over kotlin/arkui, the look it shares with harken
```

The phones sign in the way the browser does (below).

## The household

**One budget per server, and everyone in it shares it.** Whoever the
server's OpenID provider lets in is the household: every table is fed
whole to every connection (`feed` in `domain/src/lib.rs`), and nothing is
anybody's to guard. A row's `user_id` records who wrote it (a register
shows "Entered by sam"). It never decides who may do what. One role
exists, `bank`, held by the server's importer and nobody else, and only
`banks.import` asks for it. This is the household decision harken made
(`docs/plan-auth.md`, Q1), applied to money.

## The budget

The rules are a zero-based budget's, the ones YNAB made familiar
(`domain/src/budget.rs`):

- **Money that comes in is ready to assign.** An inflow for the budget's
  own category, Ready to Assign, adds to it, and so does an on-budget
  account's starting balance. Assigning money to a category in a month
  takes from it.
- **A category's money stays until it is spent.** Available is what it
  carried in, plus what the month assigned it, plus the month's activity.
- **Overspending is paid for by next month.** A category that ends a
  month below nothing starts the next at nothing, and the shortfall comes
  out of that month's Ready to Assign.
- **A target is what a month sets aside.** A category is underfunded when
  the month assigned it less than its target.

Nothing is stored except what people said: assignments and transactions.
`budget.month`, `budget.summary`, `accounts.accounts` and the rest compute
every figure. A transaction that arrives late (an import, a device that
was offline) moves every month after it, on every device, the same way.
`domain/tests/views.rs` checks every view, after each change, against a
fresh evaluation.

Accounts are on budget (checking, savings, cash, credit) or tracking only
(loan, investment). A transfer is two transactions that change together.
Between two on-budget accounts it has no category. Out of the budget, its
budget side is spending like any other. Reconciling agrees with the bank's
balance, adds an adjustment for any difference, and locks what it agreed
to.

**Not built:** YNAB's credit-card payment categories. Spending on a card
comes out of its categories as cash spending would, and paying the card
is a transfer. Splits, scheduled transactions, goals with dates, and
moving categories by drag are also missing.

## The bank, through Plaid

`domain/src/plaid.rs` is ArkOS's `money-sync` with Actual Budget
replaced by the domain:

- Plaid's `/accounts/get` and `/transactions/sync` answers are read as
  they arrive and turned into one `Import` in cents and our sign
  (`import_of`). These are `sync.rs`'s `entity` and `bank_balance`.
- `banks.import` is `actual-import`'s `runImport` as a mutator, so it
  runs on every peer against the budget as it stands when the entry
  lands. Plaid's id travels as `imported_id`, which makes a run
  idempotent. A pending transaction that posts is moved to the posted id
  in place, so a category given to it while pending survives. A
  transaction someone typed in first is matched (same account, same
  amount, within three days) instead of doubled. A reconciled one is left
  alone. An account the server's import is the first to see waits for
  somebody to choose what it feeds (the demo's bank opens its card at
  once, under the bank's name). A new transaction from a payee seen
  before gets that payee's last category and waits to be approved.
- What a bank's account feeds is the household's choice, on the Banks
  page: `banks.open` opens an account for it of the right kind (a card is
  a credit account), whose starting balance is the bank's balance minus
  the whole history the next import brings; `banks.link` points it at an
  account kept by hand, which takes what the bank has from the day the
  bank was first seen; `banks.ignore` stops following it. A choice for a
  waiting account has the next import ask Plaid from the start
  (`Bank.repull`), so nothing between the first import and the choice is
  lost, and nothing already imported doubles.
- `banks.sync` is anybody's ask that the importer run now.
- Connecting is an ask too (`domain/src/banks.rs`): `banks.connect`,
  `banks.reconnect {bank}` (Plaid's update mode, for a login that ran
  out) and `banks.disconnect {bank}`, which anybody may author and only
  the importer answers (`banks.answer`: waiting with a Plaid page's
  address, done, failed with a sentence, expired). `banks.asks` is the
  reader's own asks, newest first. `banks.failed` is what Plaid said when
  a bank's import failed, shown beside the bank (`needs_login` for
  `ITEM_LOGIN_REQUIRED` and its kin) until an import works.

The importer is the server's (below). Its tokens are secrets and never go
in the log. `domain/tests/import.rs` walks the three phases ArkOS's
end-to-end check walked Actual through, using the same mock answers.

## The server

```
MONEY_DEV_AUTH=1 cargo run -p money-server        # from rust/; 127.0.0.1:8788
cargo run -p money-iced -- --server http://127.0.0.1:8788 --user alice
nix build .#money-server .#money-web-server       # what services.money runs
```

`money-server` is harken-server's shape around money's domain
(`server/src/lib.rs`): the log on the hub, the sync socket at `/sync`,
the sign-in routes under `/auth`, `/healthz` (which says `domain <schema
hash> <version> current`, what a phone holds its build to), and the
browser client at `/`; state in the data directory. Everything is the
environment, under `MONEY_` (`money-server --help`; `Config::from_env`
lists it), which the NixOS module writes.

**Signing in.** The server is the only OpenID Connect client, a
confidential one: it holds the secret, and the provider is told one
redirect URI, `{publicUrl}/auth/callback`. Everybody else signs in
through the server: open `/auth/login?redirect=…`, come back with a
one-minute code, trade it with `POST /auth/exchange {"code"}` for a
login whose token opens the socket. The browser comes back to its own
page, the desktop to a loopback port, and a phone to `money://…` — the
scheme every money server allows. Whoever the provider lets in is the
household.

**The importer** (`server/src/importer.rs`) is a peer in the server's
process, signed in as the account `bank` and holding the `bank` role,
which the server grants it and nobody else unless `MONEY_ROLES` says so.
For each bank in the server's store (below), a run asks Plaid's
`/accounts/get` and every page of `/transactions/sync` from the bank's
cursor (`has_more` followed; started over if Plaid says the data moved
mid-way), makes one import (`import_of`) and authors `banks.import`. The
cursor is kept in the store only once the hub has confirmed the entry,
which it does once the entry is on its disk: a run cut short anywhere
before that is asked again from the old cursor, and an import said twice
changes nothing. Keys are read from their files whenever they are
needed, tokens from the store; both go to Plaid alone and are in no line
the importer says, no row, no view.

It runs once at start, on `SIGUSR1` (the module's timer, twice a day by
default), whenever somebody in the household presses **Sync** or chooses
what an account feeds (`banks.sync` writes their ask as a row, the
importer holds the log like any peer, and an ask newer than the last it
answered is answered by a run), and for one bank at once when it has just
been connected or logged in to again.

**Connecting a bank** is Plaid's Hosted Link, as ArkOS's laptop CLI did
it, run by the server: for `banks.connect` the importer creates a link
token with `hosted_link` (`/link/token/create`, products `transactions`,
`days_requested` the configured history) and answers the asker with the
page's address; it asks `/link/token/get` every five seconds until the
page is finished, trades the session's public token at
`/item/public_token/exchange`, keeps the login in the store under a name
made from the institution's (`wells-fargo`), answers done, and imports
the bank at once. Nothing has to reach the server from the browser, and
there is no redirect to register. A page lasts 30 minutes; one open when
the server stops is answered expired at the next start (the link token
is held in memory only). `banks.reconnect` is the same page in update
mode (the item's access token instead of products); `banks.disconnect`
calls `/item/remove` and drops the login.

**The store** (`server/src/store.rs`) is `banks.json` in the data
directory, written whole and moved in, mode 0600, never in the log:

```json
{
  "version": 1,
  "banks": {
    "wellsfargo": {
      "access_token": "access-production-…",
      "cursor": "…",
      "item_id": "…",
      "institution_id": "ins_…",
      "institution": "Wells Fargo",
      "added_ms": 1793000000000
    }
  },
  "answered_ms": 1793000000000
}
```

Each connection is keyed by the name the budget knows the bank by (its
`bank` row, `Linked.bank`). Only `access_token` is required: the
importer fills in `item_id`, `institution_id` and `institution` from
Plaid at its next run. Without `cursor` the next import asks from the
start (idempotent, but it brings back transactions somebody deleted); a
connection without one takes the cursor `importer.json` (where the
importer kept cursors before, `{"cursors": {"<name>": "…"}}`) has under
its name. **Back up the whole data directory**: the log, the session
key and `banks.json` together are the server, and a restore here or on
another machine with the same Plaid keys keeps every bank connected.

**Moving a connection in by hand** (a bank linked before this, its token
a secret elsewhere), with the server stopped: write `banks.json` as
above with the bank's existing name, its `access_token` and the `cursor`
from `importer.json` (or leave the cursor out and keep `importer.json`),
give it the data directory's owner and mode 0600, and start the server.
It imports only what is new since that cursor.

**`services.money`** (`server/nix/module.nix`, the flake's
`nixosModules.money`): `enable`, `package`, `web`, `port` (8788),
`address` (loopback), `publicUrl`, `oidc.{issuer, clientId,
clientSecretFile, scopes}`, `devAuth`, `redirects`, `roles`,
`plaid.{env, clientIdFile, secretFile, history}`, `schedule` (OnCalendar,
`*-*-* 06,18:00:00`), `dataDir` (`/var/lib/money`, the StateDirectory of
a DynamicUser — `/var/lib/private/money` on disk), `retainEntries`,
`openFirewall`. Every secret of the deployment's is a systemd
credential, read from `%d`. `plaid.items` and `accounts` are gone: a
configuration that still sets them fails to evaluate, saying where banks
and their accounts' choices live now.

## The window on a server

`money-iced` without features is the household's client, as harken-iced
is (`iced/src/live.rs`): it opens this device's replica of the server's
budget (`--server`, `?server=`, or whoever served the page), signs in as
above, remembers the login per server, and pumps the socket when it
rings. **Sync** there is `banks.sync`. **Banks** (in the sidebar; and
**Connect a Bank** on All Accounts) lists each connection with how its
last import went, Reconnect and Disconnect, and each of its accounts
with what it feeds: a new account of its kind, an existing one, or
nothing (`iced/src/banks.rs`). Connecting opens Plaid's page as soon as
the server answers: in the system's browser on the desktop; in a browser,
in a tab opened empty at the click (a browser opens tabs only right after
one) and pointed at the page when the answer arrives, with the page's
link on the Banks page if the tab was blocked. `money-web-server` is that
page, which `services.money` serves at `/`; `money-web` is the demo.

## The demo

```
nix build .#money-web                       # the page, as GitHub Pages would serve it
cargo run -p money-iced --features demo     # the window, from rust/
```

It opens on a seeded household: three months up to today, two people
(Alex's window, some of it entered by Sam), checking, savings, cash, a
mortgage and a retirement account, and a Chase card fed through Plaid,
whose last few days are waiting for a look. **Sync** stands in for Plaid:
the first press brings a pending coffee and a fill-up, and the second
posts the coffee with its tip in place and brings an order. After that the
bank has nothing new. Nothing leaves the window, and a reload starts from
the seed again.

The layout follows YNAB: accounts down the side; the month's categories
with Assigned, Activity and Available, and Ready to Assign across the
top; the selected category's story on the right (its target, Move Money
or Cover Overspending, Auto-Assign); a register per account with cleared,
uncleared and working balances; and Reflect, where the month's spending
went.

**What is verified:** the domain's rules, transfers, reconciliation, the
import's phases, the accounts a server names and does not follow, and
every view against a fresh evaluation (`domain/tests/`); the demo's
seed, and every button the tests press (`iced/src/tests.rs`). The server
in the process on a real port (`server/tests/`): signing in through the
routes and syncing on the socket, a phone's code sent back to
`money://` and exchanged, a forged token turned away, nobody signed in
holding `bank`, the budget and the logins across a restart, `/healthz`;
and the importer against ArkOS's mock Plaid answers on loopback: the
three phases, an ask from a member on the socket answered, the cursor
resumed across a restart and kept only after the import is in, a run
said twice changing nothing, a failed page keeping no cursor, and no key
or token in anything it says or in the log; and connecting from the app
against its scripted Hosted Link: the page's address answered to the asker
alone, the page finished, the login kept in `banks.json` (0600) and the
bank imported at once, accounts chosen for and the history asked again,
a page given up on or left to expire, a login given again in update
mode, a bank disconnected, a `banks.json` written by hand (token and
cursor alone, or the cursor in `importer.json`) importing only what is
new, and a copy of the data directory imported from on a fresh server.
The live window signs in to
a server in the process; its Sync lands as an ask, and its Banks page
connects, opens the page the server answers with once, chooses, and asks
to reconnect and disconnect
(`iced/src/tests.rs`). `services.money` is evaluated under its
configurations (`checks.money-module`). The desktop demo window has been
opened under Xvfb and each page looked at.
**What is not:** real Plaid (sandbox or production); a real OpenID
provider in front of this server (the routes are ark-auth's, which
harken's deployment uses); the module on a booted machine (the timer's
signal, the credentials, the hardening); the live window drawn, on the
desktop or in a browser (the browser build compiles for wasm32 with and
without `demo`), and the desktop's hand-off to a browser to sign in
or to Plaid's page (the browser build's tab opened at the click is
unverified in a real browser);
typing into the demo's fields under Xvfb; and a phone, which does not
exist yet.
