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
ios/, android/
           the phone apps, on ffi/ (android/README.md: Material You,
           over kotlin/arkui, the look it shares with harken)
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
  alone. The first import that sees an account opens it under the bank's
  name, with a starting balance equal to the bank's balance minus the
  imported history. A new transaction from a payee seen before gets that
  payee's last category and waits to be approved.
- `banks.link` points a bank's account at an existing account, and
  `banks.ignore` stops following one.
- `banks.sync` is anybody's ask that the importer run now.

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
For each bank it holds a token for, a run reads the token from its file,
asks Plaid's `/accounts/get` and every page of `/transactions/sync` from
the bank's cursor (`has_more` followed; started over if Plaid says the
data moved mid-way), makes one import (`import_of`) and authors
`banks.import`. The cursor is kept in `importer.json` in the data
directory only once the hub has confirmed the entry, which it does once
the entry is on its disk: a run cut short anywhere before that is asked
again from the old cursor, and an import said twice changes nothing.
Keys and tokens are read at each run, go to Plaid alone, and are in no
line the importer says.

It runs once at start, on `SIGUSR1` (the module's timer, twice a day by
default), and whenever somebody in the household presses **Sync**:
`banks.sync` writes their ask as a row, the importer holds the log like
any peer, and an ask newer than the last it answered is answered by a
run. What it answered is kept beside the cursors.

**Which accounts.** `MONEY_ACCOUNTS` (the module's `accounts`) names the
banks' accounts the way the household had them in Actual — by mask, or
Plaid's account id when two share one. A bank with any account named
follows those alone: the first import opens each under its name, and the
bank's other accounts are known (a `Linked` row, ignored) and opened
nowhere, until somebody links one to an account (`banks.link`). A bank
with none named follows every account under the bank's name. Either way
it is the first sight's say only: after that, which account a bank's
feeds is the household's. This is ArkOS's rule (unmapped accounts are
not imported) kept, with the accounts still reachable from the app.

**`services.money`** (`server/nix/module.nix`, the flake's
`nixosModules.money`): `enable`, `package`, `web`, `port` (8788),
`address` (loopback), `publicUrl`, `oidc.{issuer, clientId,
clientSecretFile, scopes}`, `devAuth`, `redirects`, `roles`,
`plaid.{env, clientIdFile, secretFile, items.<name>.accessTokenFile,
history}`, `accounts.<name>.{item, mask, accountId}`, `schedule`
(OnCalendar, `*-*-* 06,18:00:00`), `dataDir` (`/var/lib/money`, the
StateDirectory of a DynamicUser), `retainEntries`, `openFirewall`. Every
secret is a systemd credential, read from `%d`.

## The window on a server

`money-iced` without features is the household's client, as harken-iced
is (`iced/src/live.rs`): it opens this device's replica of the server's
budget (`--server`, `?server=`, or whoever served the page), signs in as
above, remembers the login per server, and pumps the socket when it
rings. **Sync** there is `banks.sync`. `money-web-server` is that page,
which `services.money` serves at `/`; `money-web` is the demo.

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
or token in anything it says or in the log. The live window signs in to
a server in the process and its Sync lands as an ask
(`iced/src/tests.rs`). `services.money` is evaluated under its
configurations (`checks.money-module`). The desktop demo window has been
opened under Xvfb and each page looked at.
**What is not:** real Plaid (sandbox or production); a real OpenID
provider in front of this server (the routes are ark-auth's, which
harken's deployment uses); the module on a booted machine (the timer's
signal, the credentials, the hardening); the live window drawn, on the
desktop or in a browser (the browser build compiles for wasm32 with and
without `demo`), and the desktop's hand-off to a browser to sign in;
typing into the demo's fields under Xvfb; and a phone, which does not
exist yet.
