# money, Designed Mac-First

A household's budget, YNAB's way, designed as a standard macOS SwiftUI app
first (ui/docs/design.md) and adapting from there. Its shape is Food
Truck's: a sidebar of sections, each a content list, a detail, a toolbar,
an inspector and a Settings scene. Only the Accounts slice is written in
ark-app today (money/app); the rest is the plan for the port.

## Scenes

- **Main window** (`WindowGroup`): the sidebar-adaptable tabs below.
- **Settings** (`Settings`): Server and Sign-In, Bank Connections,
  Household, Appearance. A window on the Mac and the desktop, a screen
  elsewhere, a dialog route in the browser.
- **Commands**: File (New Transaction ⌘N, New Account ⇧⌘N, Import
  Receipt…), View (Previous Month ⌘[, Next Month ⌘], This Month ⌘T,
  Show Inspector ⌥⌘I, Refresh ⌘R), Go (Budget ⌘1, Accounts ⌘2,
  Transactions ⌘3, Insights ⌘4).

## Sections (the sidebar, a tab bar when compact)

| Section | Content | Detail | Toolbar | Inspector |
|---|---|---|---|---|
| Budget | the month's category groups as a table (Assigned, Activity, Available) | — (the table is the page) | the month pager (principal), Assign (primary) | the selected category: target, history, notes |
| Accounts | net worth, then the accounts by shelf (Budget, Tracking, Closed) | the selected account's register | Add Account (primary), search (`/`) | the selected transaction |
| Transactions | every account's register, searchable, with saved filters in the sidebar | the selected transaction | New Transaction (primary), filter (secondary), search | the selected transaction's splits and receipt |
| Insights | the month's spending and income charts | — | the month pager | the selected category's trend |

On a compact width the split view collapses to a stack: the Accounts list
pushes the register (a list row that opens a page draws the platform's
disclosure: `ListRow(pushes:)`), and the inspector is a sheet.

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
- **Refresh** asks the bank (`Command::Native("refresh")`).

Not yet in the slice: the split view itself (the register opens as a
pushed page through the shell's navigator), the inspector, the toolbar
placements and the section's search; they wait for those parts in ark-app
(ui/docs/app.md, What Is Left).

## Keyboard

ArkUI's (ui/docs/design.md, The Keyboard): `j`/`k` through the accounts,
`l`/`Enter` into the register, `h` back to the sidebar, `gg`/`G`, `/`
to search, `Shift-F10` for a row's menu; money adds its Commands above.
