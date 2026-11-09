# money, on ArkDB

A household's budget: the accounts, the month's categories, what each
was given and spent, and the bank's transactions through Plaid. All of it
runs on ArkDB at spec v5, as harken does: one domain in plain Rust, every
figure a view kept up to date by the rows a change touched.

```
domain/    the domain, in plain Rust (spec/AUTHORING.md): src/schema.rs,
           types.rs, dates.rs; accounts.rs, categories.rs, budget.rs,
           transactions.rs; plaid.rs, the bank import; receipts.rs and
           stores.rs, the stores' receipts; migrations.rs. The demo's
           household is beside the app, in seed/
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
           over ui/kotlin, the look it shares with harken
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

**Categories are one tree** (`domain/src/categories.rs`). A category
with no parent is a heading, which the budget draws as a group. One under
a heading is an envelope: a month gives money to it, and it has Assigned,
Activity and Available. One under an envelope, at any depth, is a kind of
it (`Everyday › Groceries › Eggs`). Its spending is its envelope's, and
nothing is assigned to it. The budget draws headings and envelopes as it
did; `categories.tree` is the whole tree, and `categories.add_subcategory`
adds a kind. Groups were a table of their own until
`migrations/categories-as-a-tree.json`, whose step makes each a heading.

**A transaction can be split by amounts** (`transactions.split`): its
amount in lines, each for a category of its own, adding up to it. A line
for no category yet leaves it needing one, in the inbox. Activity, Ready
to Assign and Reflect read the lines. Giving a split transaction one
category unsplits it, and its amount changes only with its lines.

**Not built:** YNAB's credit-card payment categories. Spending on a card
comes out of its categories as cash spending would, and paying the card
is a transfer. Scheduled transactions, goals with dates, and moving
categories by drag are also missing.

## The stores' receipts

`domain/src/receipts.rs` keeps what a store says was bought, line by line,
beside what the bank says was paid:

- **Adapters** (`domain/src/stores.rs`) read an export into one
  `ReceiptImport`. Amazon's is what `amazon-orders orders --full-details
  -o json` prints. Target's is the JSON its order history loads. Costco's
  is its Orders & Purchases page's `receiptsWithCounts` JSON, warehouse and
  gas station alike (fuel in gallons). Amazon's is the library's own
  serializer. Target's and Costco's are their sites' unpublished JSON, read
  under the names they have been seen with.
- **Filing an item.** An item is filed under the category the same
  store's same item was filed under last (by its ASIN, TCIN or item
  number), or under a category whose name is in its own ("Eggs" in `KS
  ORGANIC EGGS 24CT`). Filing one (`receipts.file_items`) files every
  unfiled item with the same key. `receipts.items` lists a category's
  items from every store, with what one cost.
- **Paid by.** A receipt is paid by a transaction for the same amount, on
  budget, from the store's payee, from two days before it to four days
  after (two weeks for Amazon). Whichever arrives second makes the match,
  and `receipts.pay` says otherwise.
- **Charges.** A store that says how it charged an order, as Amazon
  does through `amazon-orders transactions` (shipment by shipment, a gift
  card taking some, a refund), gives each charge (`receipts.add_charges`).
  A receipt with charges is paid by them and not whole. Each charge finds
  the transaction of its own amount from the store's payee, three days
  before its day to five after, and each of those transactions is split
  by the receipt's items, its own amount shared. `receipts.pay_charge` and
  `receipts.pay` say by hand what paid, and `receipts.candidates` is what
  could have.
- **The split.** A paid receipt splits its transaction: its items' amounts
  summed by category, and each sum's share of what the bank took. Tax and
  rounding are spread in proportion, and the lines add up to the cent.
  Items not filed yet go where the transaction went before (its category,
  or the payee's last). All one category is just that category. The split
  is written again when the receipt is matched or an item is filed.

**Settings set the stores up** (`iced/src/stores.rs`, answered by
`server/src/setup.rs`), and nothing they are told is part of the budget.
They are calls of the server's own over the sync socket
(`domain/src/setup.rs`, ark-peer's `Config::calls`), not intents: a `Call`
frame answered by a `Reply` to that connection alone, never logged, journaled
or told to anyone else. Amazon's login, with an authenticator secret for an
account with two-step verification, is kept in `<data>/stores.json`,
readable by the server alone (`server/src/vault.rs`). It is never a row, so
it is never in the log or on a device. Settings are told only who, masked.

**A browser for the stores that want one** (`server/src/browser.rs`,
`MONEY_BROWSER`, the module's `browser.devtools`): a real, headed browser,
which people sign in to themselves from its page (`MONEY_BROWSER_PAGE`,
`browser.page`) and the server drives over the DevTools protocol. Costco's
sign-in is behind bot protection that only such a browser, with a person at
it, passes, and Amazon answers a sign-in from somewhere new with a
JavaScript challenge.

**Costco is read on its own page** (`server/src/costco.rs`). "Sign in to
Costco" in Settings opens costco.com's sign-in in the browser, and the
browser's page; a person signs in there once, and the browser stays signed
in. At each sync the server opens costco.com's Orders & Purchases in a tab
of its own, which signs itself in from that session and keeps its token in
the page; the page then asks Costco's GraphQL for the warehouse and gas
receipts of the last `MONEY_COSTCO_DAYS` (90), item by item, and the tab is
closed. The first pull, as Amazon's, reaches back to the banks' first
transaction, asked 90 days at a time as costco.com's list asks; it is
marked done in `<data>/costco-pulled`, which signing in again from Settings
clears. Nothing of the sign-in comes to the server. A browser signed out is
said so in Settings' last sync. The banks' own first pull is Plaid's
history (`plaid.history`), set when a bank is connected.

**A paper receipt is read from its photo** (`server/src/paper.rs`, the call
`stores.read_receipt`), with no model doing the reading. `receipt-ocr`
(`MONEY_RECEIPT_OCR`, the module's `ocr.package`; RapidOCR on PaddleOCR's
PP-OCRv5 models) gives what it sees at three sizes of its text detector, and
rules read each into lines, items with their quantity and weight lines,
prices paired with them in order, a "You saved" section, and the subtotal
or the balance due less tax. The readings vote on each price. What adds up
is `sure`, or `settled` by the readings' own other answers; what does not
is `check`, for a person. No digit is guessed to make it add up. Nothing is
kept: the device files the receipt as it was checked, with `add_receipts`.
The module has it on wherever the system's nixpkgs has RapidOCR
(`money/server/ocr`). `tests/paper/` is four real receipts, what named a
person or a card taken out.

**The server gathers them** (`server/src/stores.rs`) on each importer run
(at start, on its timer, on Sync). It runs `amazon-orders` (`MONEY_AMAZON_ORDERS`), for the orders and then
for how they were charged (`MONEY_AMAZON_TRANSACTIONS_ARGS`, `transactions
--days 60 -o json`), signed in by the login given in Settings, or by
`MONEY_AMAZON_USERNAME_FILE`, `MONEY_AMAZON_PASSWORD_FILE` and
`MONEY_AMAZON_OTP_SECRET_FILE` when the server is given them, and told the
browser (`AMAZON_ORDERS_CDP`) for Amazon's challenge. The login goes to
that program alone, with its session kept in `<data>/amazon-orders`; what
Amazon asked or refused is said, not `Aborted!`. A login's first pull
reaches back to the banks' first transaction — the orders a year at a time
(`orders --year`), the charges as many days back — and is marked done for
that login (`<data>/amazon-orders/pulled`); each sync after asks for the
last days alone. It reads Costco in the
browser, as above. It reads a folder of exports (`MONEY_RECEIPTS_DIR`, else
`<data>/receipts`: `target/`, `costco/`, `amazon/`), moving each file to
`imported/`, or to `failed/` when it does not read. Every server runs it,
for that folder. The module's options are `amazon.{package, args,
usernameFile, passwordFile, otpSecretFile, transactionsArgs}`,
`browser.{devtools, page}`, `costco.days` and `receiptsDir`. Target has no
program like `amazon-orders`, so its exports are saved from the browser
into the folder, as Costco's may be. For Costco, `download/download_costco_receipts.js` in
ankurdave/beancount_import_sources, run in the console of a signed-in
costco.com page, saves the array the adapter reads
(`domain/tests/stores/costco-receipts-script.json` is its shape).

## Filters and formulas

**A filter says which transactions** (`domain/src/filters.rs`): conditions
every one of which must hold — the accounts (none is every account; or
only those on the budget), text the payee or the bank's line holds (any
case), the categories ("no category" among them), which way the money
went (either, in, out), transfers in or left out, only what needs a look
(not approved, or on budget for no category), and optionally a range of
days. `Filter::matches` is the only matcher: every register is the
register for a filter (`transactions.filtered {saved | filter, month}`,
with `transactions.filtered_total` its sum), and the built-in ones are
filters too — All Accounts (no conditions), Needs a Look (the inbox's
condition) and one per account. The phones' `transactions.register` and
`transactions.inbox` are those built-in filters through the same matcher.

**Saved filters** (`SavedFilter`, appended to the schema) have a name,
unique in any case, and a place: `filters.save_filter`, `rename_filter`,
`change_filter`, `move_filter`, `delete_filter`, and `filters.saved`,
which lists them with what each adds up to (ever, within its own days).
A category deleted is replaced in a filter's conditions by the one its
transactions moved to (or "no category").

**A formula assigns a category** (`domain/src/formulas.rs`), as a
spreadsheet's cell is worked out: a sum of terms, each `factor ×
SUM(saved filter over the month)` — "10% × SUM(ACME paychecks)". A factor
is kept exactly as a ratio (typed "10%", "0.1" or "1.5x"); the terms are
added exactly and the month's total rounded once, to the cent, half away
from zero. A formula is a version from a month on (`Formula`, appended to
the schema; `categories.set_formula`, `categories.clear_formula`, the
views `categories.formulas` and `categories.preview_formula`): months
before keep what was typed, and removing one is a manual version. While a
month follows one, `assign` and `move_money` refuse it, auto-assign
passes it by, and everything else counts its amount as assigned. Ready to
Assign and a card's payment category have none. A saved filter a formula
sums (in any version) is not deleted — the deletion is refused, naming
the categories; were one gone anyway, its term would count nothing.
Changing a filter's conditions moves every month the formula covers.

**Formulas replaced rules.** The `Rule` table is still in the schema and
is read by nothing: a rule saved before this assigns nothing now (none
are migrated), and a category deleted takes its old rows with it. The
phones read `BudgetLine.rule` (the formula said) and `ruled` unchanged.
A phone built before still sends `categories.set_rule`, kept for it: it
saves (or reuses) a filter of the rule's conditions — "Income matching
acme in Checking" — and a one-term formula over it, said as the rule was.

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
- `banks.add_accounts {bank}` is Plaid's update mode with its account
  picker, for an account a connected bank did not share at first: each
  account picked there opens at the next import as `banks.open` would.
- A bank connected (`banks.connect`) for an institution already here is
  a second login to it, and folds into the first at its first import:
  each of its accounts whose twin (same type, subtype and last digits)
  at the other login has a choice takes that choice (it feeds the same
  account from the day it was first seen, or is ignored); its accounts
  with no twin were picked in Plaid's picker and open with their whole
  history; and the login every choice of which it now carries is
  replaced (`Bank.replaced_by`): the importer removes it at Plaid, drops
  it from `banks.json` and says `banks.retired`, which forgets it as a
  disconnection does. Only that first import folds; a bank whose
  accounts have no twin, or another institution's, is never touched.
- Where two connections feed one account (that fold, or a waiting
  account linked by hand to an account another login feeds, which
  `banks.link` allows), a transaction one imports that the other already
  imported (same amount, within three days, not yet matched by the
  importing one) takes the importer's id instead of being doubled
  (`Txn.imported_by` says whose copy a row is). Before the day the
  account was taken over, only a copy still pending is matched (it
  posts through the new login).

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
rings. **Sync** there is `banks.sync`. **Settings**, as harken has them
(the account button at the top right: its menu opens Settings and signs
out; `#settings`), has two sections. **Banks** (also reached by **Connect
a Bank** on All Accounts, and **Banks need a look** there when one does)
lists each connection with how its last import went (a dim line), Reconnect (only when its login ran out), Add accounts and Disconnect, and each of its accounts
with one control for what it feeds, a menu: a new account of its kind (the cursor on the account another login to the bank feeds from the same last digits, when there is one), every account here, or
nothing (`iced/src/banks.rs`); **Account** says who is signed in where,
with Sign out (`iced/src/settings.rs`). Connecting opens Plaid's page as soon as
the server answers: in the system's browser on the desktop; in a browser,
in a tab opened empty at the click (a browser opens tabs only right after
one) and pointed at the page when the answer arrives, with the page's
link in Settings if the tab was blocked. `money-web-server` is that
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

Beside the budget, **Receipts** lists the stores' receipts with what paid
for each and how many of its items are filed. One opened shows its items,
and each is filed from the dropdown the register uses, which offers an
envelope's kinds beneath it. Items not filed yet are gathered at the top,
and the sidebar counts them. A transaction opened in the register can be
**split**: lines of a category and an amount each, with what is left to
give one. The inspector lists a category's kinds and adds one.
An opened receipt lists its charges too, and "Paid by" (a receipt's, or a
charge's) opens what could have paid it, to say which did.
**Settings** has an Amazon section beside its banks, which sends you to
the server's own page to set up Amazon. The
demo's household has kinds (Eggs, Produce, Cleaning) and a Target and a
Costco receipt, matched and split.

The layout follows YNAB: down the side, Budget and Reflect, All
Accounts (with what needs a look), each account under On Budget,
Tracking and Closed, and the saved filters under Filters with their
totals; the month's categories
with Assigned, Activity and Available, and Ready to Assign across the
top; the selected category's story on the right (its target, Move Money
or Cover Overspending, Auto-Assign, Assign by formula: a row per term,
each a saved filter picked from a menu and a factor, Open for the
filter's register in the month, a live preview, Save and Remove); a
register for any filter — an account's is the filter for that account,
with its cleared, uncleared and working balances, Reconcile, Rename and
Sync — whose bar shows its conditions as chips to change (the address bar
says `#account/Checking`, `#accounts`, `#inbox`, `#filter/<name>`, or
`#filtered` for one not saved), what it adds up to, and "Save as filter…";
and Reflect, where the month's spending went.

**What is verified:** the domain's rules, transfers, reconciliation,
every filter condition alone and together, the built-in filters against
what the registers and the inbox showed, saved filters and their totals,
formulas (terms, factors, rounding once, history, refusals, cards, a
filter in use), the
import's phases, the accounts a server names and does not follow, the
tree, splits, receipts, charges and their adapters, the migration of
groups into the tree (from the schema a server stands on, and one older),
and every view against a fresh evaluation (`domain/tests/`); the demo's
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
a server in the process; its Sync lands as an ask, and its Settings (from the account menu)
connects, opens the page the server answers with once, chooses, and asks
to reconnect and disconnect
(`iced/src/tests.rs`). `services.money` is evaluated under its
configurations (`checks.money-module`). The desktop demo window has been
opened under Xvfb and each page looked at.
**What is not:** real Plaid (sandbox or production); real
`amazon-orders` against Amazon, and Target's and Costco's JSON as their
sites serve it today; the phones built against the regenerated types; a real OpenID
provider in front of this server (the routes are ark-auth's, which
harken's deployment uses); the module on a booted machine (the timer's
signal, the credentials, the hardening); the live window drawn, on the
desktop or in a browser (the browser build compiles for wasm32 with and
without `demo`), and the desktop's hand-off to a browser to sign in
or to Plaid's page (the browser build's tab opened at the click is
unverified in a real browser);
typing into the demo's fields under Xvfb; and a phone, which does not
exist yet.
