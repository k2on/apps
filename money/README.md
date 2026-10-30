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
iced/      the demo client, desktop and browser, on arkui and ark-peer:
           a seeded household on a peer that is its own authority
ffi/       the library the phones load (ffi/README.md): ark-ffi with the
           domain linked in, typed views, the demo, a server in the process
ios/, android/
           so far only the typed Swift and Kotlin generated for ffi/
```

There is no server and no phone app yet. The domain is written for one
(below), the client is the demo, the way harken's demo is, and the
phones' library is ready for the apps to be built on.

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

The HTTP client and the access tokens belong to a server's importer,
which this tree does not have yet. The tokens are secrets and never go in
the log. `domain/tests/import.rs` walks the three phases ArkOS's
end-to-end check walked Actual through, using the same mock answers.

## The demo

```
nix run .#money-iced           # the window
nix build .#money-web          # the page, as GitHub Pages would serve it
cargo run -p money-iced        # from rust/
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
import's phases, and every view against a fresh evaluation
(`domain/tests/`); the demo's seed, and every button the tests press
(`iced/src/tests.rs`). The desktop window has been opened under Xvfb and
each page looked at: the budget, a category's inspector, a register
before and after Sync, All Accounts, Reflect, and adding an account.
**What is not:** typing into its fields there (the X server had no window
manager to give it the keys; the tests drive the same messages instead),
and the browser build, which compiles for wasm32 but has not been drawn.
