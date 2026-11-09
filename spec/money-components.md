# money's own components

What money draws that is money's, so stays in the app — the same component,
the same inputs, in every shell (Android `money/android`, iPhone
`money/ios`, desktop `money/iced`), and what a web client builds. Everything
generic they are made of is ArkUI's (`ui/docs/parts.md`): a grouped
card, a section's heading, a pill, a tile, a badge, a pager, a progress bar.

| Component | Inputs | Made of | Android | iPhone | Desktop |
|---|---|---|---|---|---|
| HeroCard | caption, cents, tint, note, onClick?, footer | — | `HeroCard` | `HeroFigure` | the budget's ready box (`look::ready`) |
| HeadingTotal | cents | SectionHeader's trailing | `HeadingTotal` | the section header's total | a group row's totals |
| KindTile | account kind, side | Tile | `KindTile`, `kindColor` | `AccountMark` | the sidebar's glyph (`view::glyph`) |
| MonthPager | month, stepMonth, showThisMonth | Pager | `MonthPager` | `Pager` in `BudgetView` / `InsightsView` | `parts::pager` in the budget and Reflect |
| AccountRow | account (name, kind, balance, unapproved) | ListRow-like, KindTile, Badge | `AccountRow` | `AccountRow` | sidebar entry |
| TxnRow | transaction, showAccount, divided, onClick | — | `TxnRow` | `TxnRow`, `TxnButton` | register table row |
| DayHeader | day | — | in `RegisterScreen` | `DayHeader` | register's day line |
| CategoryRow (BudgetRow) | budget line, month | Pill, ProgressBar | `CategoryRow` (BudgetScreen) | `CategoryRow` | budget table row |
| CategoryInspector | budget line | Pill, ProgressBar, Stat | `CategorySheet` | `CategoryView` | the inspector (`view_inspector`) |
| SpendingDonut | spending by group, total | Donut | `InsightsScreen` | `InsightsView.donut` (selectable) | — (bars only) |
| PairedBars | group, assigned, spent | — | `PairedBars` | `InsightsView.bars` | — |
| PaperReceipt | receipt (store, items, total) | — | `Receipts.kt` | `PaperReceipt` | `receipt.rs` |
| CategoryPicker | ledger, title, none?, onPick | SearchField | `CategoryPicker` | `CategoryPicker` | category dropdown |
| TransactionSheet | transaction draft | AmountField | `TxnSheet` | `TransactionSheet` | the transaction form |
| BankRow / AskRow / LinkedRow | bank connection, ask, linked account | GroupCard, ListRow | `SettingsScreen` | `BankRow`, `AskRow`, `LinkedRow` | `banks.rs` |
| CaughtUp | needle | Empty | `ActivityScreen` | the review list's empty state | — |

## What each reads from the core

The components hold no rule of their own: each reads what it says from the
domain's records and their methods (`money_domain::words`, `accounts`,
`transactions::Draft`, `banks::follow`; UniFFI's methods of the same names
on the phones), and draws the core's `Tone` in ArkUI's `Tint` by one small
map per shell (Android `Tone.tint()` in `Theme.kt`, iPhone `Tone.tint` in
`Theme.swift`, desktop `look::Looks`): neutral, positive, caution for
short and on a card, negative for overspent.

| Component | Reads |
|---|---|
| HeroCard (Ready to Assign) | `Summary.tone()`, `readyWords()`, `underfunded` (what Auto-Assign funds), `overspent` |
| CategoryRow | `BudgetLine.tone()`, `meter()` (share and tone of the bar), `story(style)`, `captions(style)`, `isPayment()`, `ruleWords()`, `spoken(style)` |
| CategoryInspector | the row's, and `toCover()`, `toTarget()`, `assignable()`, `onCard()` |
| AccountRow | `AccountLine.subtitle()`, `accountKind()`; the list from `accountShelves` (Budget, Tracking, Closed), `netWorth` |
| TxnRow | `TxnLine.needsCategory()`, `bankSaid()` |
| TransactionSheet | `Draft` (`newDraft`, `TxnLine.draft()`, `withPayee`, `withCategory`, `withTransfer`, `problem()`, `toChange()`), `suggestPayees` |
| BankRow / AskRow / LinkedRow | `BankLine.title()`, `status(now)`; `AskLine.said(now)`, `closes(now)`, `isOpen()`; `askShown`; `LinkedLine.title()`, `kindWords()`, `opensWords()`; `linkableAccounts`, `bankAccounts` |
| PaperReceipt | `storeName`, `ItemLine.quantityWords()`; attach from `unmatchedReceipts(receipts, amount, day)` |
| Register search | the core's view `MoneyView.search(account, text)` (payee, the bank's words, memo, category, account, or the amount as written) |

Not yet one component in all three shells: the composables and views are
still each shell's own layout of these readings (the names in the table at
the top); the budget's drag to reorder stays per shell too
(`arkui::outline` is iced's widget state, not a UI-free model to share).
