# money's own components

What money draws that is money's, so stays in the app — the same component,
the same inputs, in every shell (Android `money/android`, iPhone
`money/ios`, desktop `money/iced`), and what a web client builds. Everything
generic they are made of is ArkUI's (`docs/arkui-parts.md`): a grouped
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
