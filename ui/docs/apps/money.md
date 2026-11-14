# money, Designed Mac-First

A household's budget, YNAB's way, designed as a standard macOS SwiftUI app
first (ui/docs/design.md) and adapting from there. Its shape is Food
Truck's: a sidebar of sections, each a content list, a detail, a toolbar,
an inspector and a Settings scene. Only the Accounts slice is written in
ark-app today (money/app); the rest is the plan for the port.

## Scenes

- **Main window** (`WindowGroup`): the sidebar-adaptable tabs below.
- **Settings** (`Settings`): General (the server and sign-in, Amazon,
  Costco, the display), Household, Connected Banks — one pane at a time. A window on the Mac and the desktop, a screen
  elsewhere, a dialog route in the browser.
- **Commands**: File (New Transaction ⌘N, New Account ⇧⌘N, Import
  Receipt…), View (Previous Month ⌘[, Next Month ⌘], This Month ⌘T,
  Show Inspector ⌥⌘I, Refresh ⌘R), Go (Budget ⌘1, Accounts ⌘2,
  Transactions ⌘3, Insights ⌘4).

## Sections (the sidebar, a tab bar when narrower)

Matched to the universal SwiftUI reference (`reference/universal-budget`
on the design-ref branch): its Mac shots at 1440×900, its iPad upright
ones, its iPhone ones.

| Section | Content | Toolbar | Inspector |
|---|---|---|---|
| Budget (title: the month) | Ready to Assign (`Headline`, Auto-Assign), the groups with their totals (tiny column titles on the first), rows with a goal bar, what needs attention in words, Assigned / Activity and the Available `StatusBadge` (the badge alone when compact) | ‹ › (navigation), Auto-Assign, the inspector's toggle | Category Details |
| Accounts | Net Worth's `SummaryCard` (assets, liabilities, twelve weeks' sparkline), Budget Accounts and Tracking Accounts with totals and notes | Sync, Add Account | — |
| Transactions (badged with what waits) | the review bar (n new from your banks · Review · Approve All), the table (Date, Payee with the review dot, Category — orange Needs Category —, Account, Memo, Amount with its `+`, Cleared), striped; two-line rows when compact | Filter, New Transaction, the inspector's toggle, search | Transaction Details (the receipt) |
| Insights | Income, Spending, Kept tiles; Spending by Category, Income vs Spending (three months), Net Worth | ‹ › | — |

Each open account is a place of its own in the sidebar under Budget
Accounts or Tracking Accounts (`tab_group`), badged with what waits in it;
its tag is `accounts/<id>`, a child of Accounts, so on a compact width the
Accounts tab is lit and the register is pushed onto it. A register shows
Cleared / Uncleared / Working Balance (`FigureStrip`) and its bank's sync.
Who is signed in is at the sidebar's foot with Settings, never over the
toolbar.

## The Accounts Slice (Written)

- **Content**: Net Worth (the page's figure); All Accounts; a section per
  shelf, its total at its heading's end (none for Closed); Bank
  Connections; Add Account. Each account row: the kind's mark, its name,
  its bank and last digits (or its kind), a review count, its balance.
- **A row's context menu** (right click, long press, the menu key), in this
  order: Rename, Reconcile… (open accounts), Copy Balance, Close Account /
  Reopen (destructive while open).
- **Add Account** (a sheet, Cancel and Add Account in its bar on Apple
  platforms, a button under the form elsewhere): Name and Type (the
  system's picker; chips on Android and the desktop), Balance (the
  iPhone's amount entry; a typed field elsewhere), In the Budget with its
  note. A new account is dated with the session's day (the demo's in the
  demo).
- **Refresh** asks the bank (`Command::Request(MoneyRequest::Refresh)`).

## The Shell (Written)

`money/app/src/shell` is the design above in ark-app's shell vocabulary:
`MoneyShell` (its state: the section, the inspector, the search, a pushed
detail), `money_shell` (the four sections, each `toolbar` over
`searchable`/`split_view`/`inspector` and a hosted screen), `money_settings`
(the four panes) and `commands()` (File, View, Go). Generated as
`MoneyShellView`, `MoneySettingsView`, `MoneyCommands` (Swift) and
`MoneyShell`, `MoneySettings`, `moneyCommands` (Kotlin); run on the phones
through money-ffi's `ShellApp`; drawn on the desktop by
`arkui::view::shell` (money/iced/src/sections.rs).

The inspectors are Rust's (`money/app/src/shell/inspect.rs`, the views
`category_inspector` and `transaction_inspector`): the shell holds the
month's lines and every account's register, the platforms tell it what
is selected (`InspectCategory`, `InspectTransaction`; a launch's
`-screen category:vacation -inspector 1` does too), and Category Details
is the reference's — a header card (the available in its status badge,
what needs attention, the group; Assign the target's rest, Cover
Overspending From), the month's figures, the goal (money's domain has one
kind, a target a month, so the reference's Goal picker of kinds and its
date are not there) and the month's transactions with Show All in
Transactions; Transaction Details its payee and amount, account, date,
status and memo (no split or receipt items yet). With nothing selected
the platform's own placeholder is hosted. Category Details is the only
one on every client: a category opened from the budget (a row, a link, a
launch) is the shell's inspector on the phones too — a pane beside the
budget when regular, a sheet when compact — and the phones' hand-written
category page and sheet are gone.

What each section shows is still the platform's own screen, hosted by
name (`hosted("budget")`): Accounts' list is the generated Accounts
slice; Budget, Transactions, Insights and Settings' Server pane are the
hand-written screens; the registers' detail beside Accounts is the iced
window's register (a placeholder on the phones), and the inspectors and
the Household and Appearance panes are placeholders. A command only a
hosted screen can carry out (New Transaction, the month) comes back to
the platform typed (`Command::Request(MoneyRequest::…)`). Adding a
transaction is the toolbar's New Transaction on Budget and Transactions,
on every client; the phones have no floating button over the rows.

## Keyboard

ArkUI's (ui/docs/design.md, The Keyboard): `j`/`k` through the accounts,
`l`/`Enter` into the register, `h` back to the sidebar, `gg`/`G`, `/`
to search, `Shift-F10` for a row's menu; money adds its Commands above.
