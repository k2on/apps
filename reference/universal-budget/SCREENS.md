# Screens and launch arguments

Every screen can be opened directly with launch arguments, so CI can take
screenshots without UI automation. The sample data is fixed (the household's
"today" is 18 November 2026), so every capture is identical run to run.

Arguments are `-key value` pairs, read through `UserDefaults` (its argument
domain), so they also work from an Xcode scheme's "Arguments Passed on
Launch" and from `xcrun simctl launch <device> com.example.universalbudget -screen budget`.
They apply to the first window only; windows opened later start fresh.

## Arguments

| Argument | Values | Effect |
|---|---|---|
| `-screen` | see below | The screen to open. Default `budget`. |
| `-inspector` | `1` / `0` | Opens the inspector on Budget, Transactions and account registers. With `category:` or `transaction:`, the item is shown in the inspector instead of being pushed. On iPhone the inspector is a sheet. |
| `-columns` | `all` | iPad: show the sidebar instead of the tab bar. (Mac always shows the sidebar; iPhone always shows the tab bar.) |
| `-month` | `2026-09` … `2027-02` | Budget month to open. Default: the sample's current month, `2026-11`. |
| `-search` | text | Initial search text on Transactions, e.g. `-search greenleaf`. |
| `-appearance` | `light` / `dark` / `system` | Overrides the Appearance setting. |
| `-settingsTab` | `general` / `household` / `banks` | Pane to open with `-screen settings`. |
| `-window` | `1440x900` | macOS: initial window size. |

### `-screen` values

| Value | Opens |
|---|---|
| `budget` | Budget for the month. |
| `category:<id or name>` | A category's detail, e.g. `category:dining` (overspent), `category:vacation` (underfunded goal), `category:groceries`. Pushed, or in the inspector with `-inspector 1`. |
| `accounts` | Accounts list with net worth. |
| `account:<id or name>` | An account register, e.g. `account:checking`, `account:visa`, `account:retirement`, or by name (`account:"Rewards Visa"`). |
| `reconcile:<account>` | A register with the Reconcile sheet open, e.g. `reconcile:checking`. |
| `add-account` | Accounts with the Add Account sheet. |
| `transactions` | All transactions. |
| `review` | Transactions filtered to the five bank imports waiting for review (three need a category). |
| `transaction:<key>` | A transaction's detail. `<key>` is a numeric id (1 is the oldest), or `receipt` (the split grocery run with an itemised receipt), `split`, `transfer`, `review` (newest unapproved), or a payee (`transaction:Cinema`). |
| `edit-transaction:<key>` | Transactions with the editor sheet open on that transaction, e.g. `edit-transaction:receipt`. |
| `add-transaction` | Transactions with a new, empty transaction sheet. |
| `insights` | Reports. |
| `settings` | Settings: a sheet on iPhone and iPad; the Settings window on Mac. |

## Recommended captures

For a stable Mac window, add `-ApplePersistenceIgnoreState YES` so macOS
doesn't restore a previous window frame, and `-window 1440x900`.

### iPhone, portrait (e.g. iPhone 16 Pro)

1. `-screen budget` — Ready to Assign, groups, available badges, tab bar.
2. `-screen category:dining` — an overspent category: cover-overspending menu, goal, activity.
3. `-screen accounts` — net worth, budget and tracking accounts.
4. `-screen account:visa` — a register in compact width (two-line rows), cleared/working balances.
5. `-screen review` — the review queue as search tokens.
6. `-screen transaction:receipt` — split, itemised receipt.
7. `-screen add-transaction` — the editor sheet with the number pad.
8. `-screen insights -appearance dark` — charts stacked in one column, dark mode.

### iPad, landscape (e.g. iPad Pro 13")

1. `-screen category:vacation -inspector 1 -columns all` — sidebar, budget columns with inline Assigned fields, category inspector.
2. `-screen transaction:receipt -inspector 1` — table with all columns and the transaction inspector, tab bar at the top.
3. `-screen account:checking -columns all` — register with balances, sidebar listing every account.
4. `-screen insights -columns all` — charts in a two- or three-column grid.
5. `-screen edit-transaction:receipt` — the editor as a form sheet.
6. `-screen settings -settingsTab household` — settings sheet.

### iPad, portrait

1. `-screen budget` — tab bar at the top, budget columns.
2. `-screen transactions` — table in regular width.
3. `-screen category:dining -inspector 1` — inspector beside the list in a narrower window.
4. `-screen review -appearance dark` — review queue, dark.

### Mac, window 1440x900

Base arguments: `-window 1440x900 -ApplePersistenceIgnoreState YES`.

1. `-screen category:vacation -inspector 1` — sidebar with accounts, budget columns, inspector.
2. `-screen transactions` — the full table; the Transaction menu shows the keyboard commands.
3. `-screen transaction:receipt -inspector 1` — table and inspector with the itemised receipt.
4. `-screen account:checking` — register with cleared / uncleared / working balances.
5. `-screen review` — tokens in the toolbar search field.
6. `-screen insights` — charts in a three-column grid.
7. `-screen add-transaction` — the editor sheet.
8. `-screen settings -settingsTab banks` — the Settings window (opens beside the main window).

Dark variants: append `-appearance dark` to any of the above.
