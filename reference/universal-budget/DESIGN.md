# Budget: design

A household envelope budget ("give every dollar a job") as one SwiftUI
target for iPhone, iPad and Mac. The aim is the shape of Apple's own
multiplatform apps and sample code: one set of views, the system containers
doing the adapting, and platform checks only where the platforms genuinely
differ.

> **Unverified.** This was written on Linux without Xcode. None of it has
> been compiled, run or seen on a device. See [What is unverified](#what-is-unverified).

## Structure

```
Sources/
  App/        BudgetApp (scenes), ContentView (tab/sidebar root), SceneModel
              (per-window navigation), BudgetCommands (menu bar), LaunchOptions
  Model/      Month, Models, BudgetStore (@Observable, all arithmetic), SampleData
  Budget/     BudgetView (+ Ready to Assign, rows, auto-assign), CategoryDetailView
  Accounts/   AccountsView (+ net worth), AccountRegisterView, AccountSheets
  Transactions/ TransactionsView, TransactionsTable, TransactionDetailView,
              TransactionEditor, ReceiptViews, FilterToken
  Insights/   InsightsView (Swift Charts)
  Settings/   SettingsView (window on Mac, sheet on iOS)
  Shared/     AmountViews (money), CategoryPickers, Platform (the few #ifs)
```

- **Data**: one `@Observable BudgetStore` for the app, shared by every
  window and the Settings window, injected with `.environment(store)`. All
  budget arithmetic is in the store (Foundation only), so views only display.
- **Navigation**: one `@Observable SceneModel` per window: selected tab,
  a navigation path per tab, the budget month, selection, the inspector, the
  one sheet, search text and tokens. Two Mac windows or two iPad windows can
  show different months. Menu commands reach the frontmost window's model
  through `focusedSceneValue`.

## Navigation and why

Four sections people move between constantly: **Budget**, **Accounts**,
**Transactions**, **Insights**. Peers used daily are what a tab bar is for on
iPhone, and since iOS 18 / macOS 15 `TabView` with `.sidebarAdaptable` gives
exactly the Apple pattern from one declaration:

| | Root | Accounts |
|---|---|---|
| iPhone | Tab bar, four tabs | Accounts tab → list → push register |
| iPad | Floating tab bar at the top; the person can switch it to a sidebar (`-columns all` starts with it) | Also listed in the sidebar, under **Budget Accounts** and **Tracking Accounts** |
| Mac | Sidebar | Each account is a sidebar item, like mailboxes in Mail |

Account tabs are in `TabSection`s marked `.defaultVisibility(.hidden, for: .tabBar)`:
they exist in the sidebar but never crowd the iPhone tab bar. Each tab
(section or account) owns a `NavigationStack`, with every destination
registered once (`routeDestinations()`), so pushing works the same on all
three.

**Settings** is a `Settings` scene on Mac (app menu, ⌘,; a window with a tab
per pane, as Mac apps do) and a sheet from a toolbar button on iPhone and
iPad, where there is no settings scene. The panes are the same views.

**Modal tasks** (new / edit transaction, add account, reconcile) are sheets
with a Form, Cancel and Add/Save in `.cancellationAction` /
`.confirmationAction`. One `sheet(item:)` at the window root; the
`SheetRoute` enum lists them.

### Considered and rejected

- **`NavigationSplitView` as the root** (Food Truck's choice). It adapts
  well on iPad and Mac, but on iPhone it collapses to a list you must back out
  of to change section; a budget app is used in short visits that jump
  between sections, which is the tab bar's job. `.sidebarAdaptable` gives the
  sidebar where it belongs without giving up the tab bar.
- **A three-column split for Accounts → register → transaction**. The
  register is a wide table on Mac; squeezing it into a content column wastes
  it. The transaction goes to the inspector instead.
- **A Settings tab**. Settings isn't a daily destination; it would cost a tab
  on iPhone and be un-Mac-like. A toolbar button on iOS, the Settings scene
  on Mac.

## How it adapts

The rule is to let containers adapt and keep explicit checks few. The
complete list of platform/width checks:

1. `@IsCompact` (Shared/Platform.swift): horizontal size class on iOS, always
   false on Mac. Used by the budget row (columns vs. one number) and by the
   transaction table's first column (see below).
2. `dynamicTypeSize.isAccessibilitySize`: the budget drops its columns and the
   Ready to Assign card stacks vertically at accessibility sizes.
3. `#if os(macOS)` for the Settings scene, `openSettings`, the window size,
   and the Settings view's tab layout; `#if os(iOS)` for the Settings toolbar
   button, `keyboardType`, and inline navigation titles. Image decoding for
   receipts (`UIImage`/`NSImage`) and sheet sizing on Mac are wrapped once in
   Platform.swift.

Everything else is the system:

- **Budget**: one `List` with sections. Regular width shows Assigned /
  Activity / Available columns (Assigned editable in place); compact width
  shows just the Available badge, and the rest is a tap away.
- **Transactions / registers**: a `Table`. On Mac and in regular-width iPad
  it has Date, Payee, Category, Account, Memo, Amount, Cleared columns, all
  sortable where meaningful, multi-select. In compact width SwiftUI shows only
  a table's first column, so that column draws a two-line row when compact
  (the pattern Apple's Food Truck sample uses for its orders table). iPad Split
  View and Slide Over get the compact layout automatically.
- **Inspector**: `.inspector` beside Budget, Transactions and registers. On
  Mac and iPad it's a trailing column; in compact width SwiftUI presents it as
  a sheet.
- **One "open" rule** (`SceneModel.open`): choosing a row shows it in the
  inspector if the inspector is open, otherwise pushes it. iPhone therefore
  pushes; iPad and Mac with the inspector open get a live detail pane; Mac
  with it closed pushes on double-click/Return. Lists use
  `contextMenu(forSelectionType:menu:primaryAction:)` so tap (iOS) and
  double-click (Mac) both go through it, and the same context menu serves
  right-click, long-press and multi-selection.
- **Insights**: a `LazyVGrid` with `.adaptive(minimum:)` columns: one on
  iPhone and narrow windows, two or three on iPad and Mac. Stat tiles use
  `ViewThatFits` to go from a row to a stack.
- **Narrow Mac windows** behave like a regular-width iPad (Mac windows are
  never compact); the table scrolls horizontally rather than collapsing, which
  is what Mac tables do.

## Screens

- **Budget**: title is the month; ‹ › in the toolbar's `.navigation`
  placement, "This Month" when away from it. **Ready to Assign** card at the
  top: the amount in large rounded figures, green when there's money to
  assign, red ("Assigned Too Much") when negative, plus "2 overspent · 2
  underfunded", and the **Auto-Assign** menu (Underfunded, Assigned Last
  Month, Spent Last Month, each showing its total; Reset). Group headers carry
  group totals. Rows: name, goal progress bar, an attention line ("Overspent
  by $44.10", "$173.25 more needed this month"), and the Available badge.
  Context menu: Show Details, Assign Amount Needed, Cover Overspending From ›
  (moves money from categories with money available), Show Transactions.
- **Category detail** (inspector or pushed): available, the month's
  carried-over / assigned / activity / available with assigned editable,
  one-tap "Assign $X" for an underfunded goal, cover-overspending, goal
  editor (monthly amount, target balance, target by date) with progress, and
  this month's transactions.
- **Accounts**: net worth card (assets, liabilities, a sparkline and a link to
  the report), Budget Accounts and Tracking Accounts sections with totals and
  explanatory footers, rows with type icon, bank and status, balance, and a
  badge for transactions to review. Add Account (+), Sync, pull to refresh.
- **Register**: the transaction table for one account under a bar with
  Cleared + Uncleared = Working Balance and the bank's sync status ("Harbor
  Credit Union · Synced 18 Nov 2026 at 1:00 PM", or an orange "Sign in again"
  for a broken connection) with a "3 to review" link. Toolbar: Sync,
  Reconcile, New Transaction, inspector.
- **Reconcile**: "Is your cleared balance $X?" Yes/No; No asks for the
  statement balance and shows the difference, Finish records an adjustment and
  locks cleared transactions (reconciled transactions show a lock and can't be
  un-cleared).
- **Transactions**: all accounts. Bank imports waiting for review have a blue
  dot (like unread mail) and a banner "5 new transactions from your banks"
  with Review and Approve All. Uncategorised imports show an orange "Needs
  Category" menu right in the cell, so reviewing is: look, pick a category,
  ⌘↩.
- **Search and filters**: `.searchable` with **tokens**. Filters (account,
  category, date range, inflow/outflow, needs review, uncategorised, has
  receipt) are tokens in the search field, where they're visible and
  removable; the toolbar Filter menu and the search suggestions both add
  tokens. Text matches payee, memo, account, category and amount.
- **Transaction detail**: payee and amount large, "Imported from …" with an
  Approve button (disabled until it has a category), account / transfer
  account (linked), category picker, date, status, memo, split lines, receipt.
- **Editor**: Expense / Income / Transfer segmented control, amount (number
  pad), account, payee with recent payees (choosing one fills the category it
  had last time), category grouped by section (Ready to Assign for income),
  date, cleared, memo; **Split** into categories with a running "left to
  split"; **Receipt**: photo from Photos (`PhotosPicker`, works on Mac too)
  and itemised lines with quantity and price. Transfers ask for the other
  account and only show a category when money leaves the budget (budget →
  tracking, e.g. a mortgage payment). Saving a transaction approves it.
- **Insights**: Income / Spending / Kept tiles for the chosen period (toolbar
  segmented picker: This Month / Last 3 Months); Spending by Category
  (horizontal bars, largest first, top eight plus "Other", single colour,
  values labelled); Income vs Spending by month (grouped bars, legend,
  select a month for exact figures); Net Worth (line with a light area,
  selectable).
- **Settings**: General (currency with a formatted example, appearance),
  Household (name, members with roles and avatars, invite by email), Connected
  Banks (accounts per bank, last sync, problems, Sync Now / Reconnect,
  Disconnect with confirmation).

## Visual design

- **Typography**: system text styles only, so Dynamic Type works
  everywhere. Headline money (Ready to Assign, net worth, a transaction's
  amount) is `.largeTitle` bold in the **rounded** design, a friendlier face
  for big figures, as in Apple's Health and Wallet. Every amount uses
  `.monospacedDigit()` so columns align and changing numbers don't shift.
- **Colour**: a deep teal-green accent (light and dark variants in the asset
  catalog) for interactive elements, the money-in series and bars. Semantic
  colours are reserved for budget state: **green** money available, **orange**
  underfunded (more needed), **red** overspent (a filled red badge with white
  text, the one place the app shouts), **grey** zero. State is never colour
  alone: overspent and underfunded badges carry a symbol and an attention line
  in words, and inflows carry a "+".
- **Charts**: one hue for magnitude (spending by category, net worth), two
  categorical colours with a legend for income vs spending, one y-axis per
  chart, rounded bar ends, recessive grid lines, selection callouts rather
  than a number on every mark. The palette is system colours (accent and
  indigo); it was not run through a colour-vision validator.
- **Numbers and money**: `Decimal` throughout, never `Double` (charts convert
  at the edge). Formatting is `Decimal.FormatStyle.Currency` with the
  household's currency (a setting) and the person's locale (grouping, decimal
  mark, symbol position), passed down as `\.currencyCode` in the environment.
  Outflows are negative ("−$45.00") in primary colour; inflows are "+$2,340.00"
  in green. Liabilities show as negative balances, so net worth is a plain sum.
  Dates are abbreviated in tables ("Nov 18, 2026") and long in detail.
- **Empty states** use `ContentUnavailableView` with a next step: no
  accounts (Add Account), no transactions (New Transaction / connect a bank),
  search with no results (`.search(text:)`), filters with no results ("Try
  removing a filter"), review queue empty ("All Caught Up"), nothing selected
  in an inspector, no spending in a period, no connected banks.

## Keyboard and menu commands

All commands live in `BudgetCommands`: the menu bar on Mac and the ⌘-hold
shortcut overlay on iPad with a keyboard.

| Menu | Command | Shortcut |
|---|---|---|
| File | New Transaction (in the account on screen, if any) | ⌘N |
| | New Account… | ⇧⌘N |
| | New Window | ⌥⌘N |
| Edit | Find Transactions… (focuses the search field) | ⌘F |
| View | Budget / Accounts / Transactions / Insights | ⌘1 – ⌘4 |
| | Show/Hide Sidebar (system) | ⌃⌘S |
| | Show/Hide Inspector (system `InspectorCommands`) | ⌥⌘I |
| Budget | Previous / Next Month | ⌘[ / ⌘] |
| | Current Month | ⇧⌘T |
| | Auto-Assign › Underfunded, Assigned Last Month, Spent Last Month, Reset | ⌥⌘A (Underfunded) |
| Transaction | Approve / Approve All | ⌘↩ / ⇧⌘↩ |
| | Edit Transaction… | ⌘E |
| | Mark as Cleared / Uncleared | ⇧⌘C |
| | Categorize › (grouped categories) | |
| | Duplicate | ⌘D |
| | Delete | ⌘⌫ |
| | Reconcile Account… | ⌥⌘R |
| | Sync with Banks | ⌘R |

Lists and tables are keyboard-navigable (arrow keys move the selection, ⇧
extends it); the Return/double-click primary action opens the row; in the
budget, Tab moves through the Assigned fields. The editor puts focus in the
amount field (`defaultFocus`). Toolbar buttons have `.help` tooltips that name
their shortcuts. ⌘N/⌘F are taken by the app, so New Window moves to ⌥⌘N, as
in Mail.

## Accessibility

- Dynamic Type everywhere; widths that hold numbers are `@ScaledMetric`; at
  accessibility sizes the budget switches to its single-number layout and
  summary rows stack.
- VoiceOver: compact rows are combined into one element with a spoken
  summary ("Dining Out, −$44.10 available"); the Available badge reads
  "Available, −$44.10, Overspent by $44.10" (figures illustrative); icon-only buttons have labels;
  decorative emoji and icons are hidden; the review dot reads "Needs review";
  the cleared button reads its state. Charts get per-mark labels and values
  (Swift Charts adds Audio Graphs); the net-worth sparkline has a spoken
  trend.
- Colour is never the only signal (symbols, signs, words).
- Haptics: success feedback on approving (`sensoryFeedback`).
- Animation is the system default (`withAnimation`, `.numericText`
  content transitions), so Reduce Motion is honoured by the system.

## Model notes and trade-offs

- **Envelope arithmetic** (BudgetStore): available = carried over + assigned
  + activity. Positive balances roll over; overspending doesn't follow the
  category, it comes out of the next month's Ready to Assign. Ready to Assign =
  income (inflows to Ready to Assign, including opening balances) through the
  month − everything assigned through the month − overspending in earlier
  months. Assignments in future months are not subtracted from earlier months.
- **Goals**: monthly amount (needed = target − assigned); target balance by a
  date (needed = (target − carried over) ÷ months left − assigned); target
  balance without a date shows progress but is never "underfunded".
- **Credit cards** are budget accounts: spending on the card is categorised
  like any spending, and a card payment is a transfer between budget
  accounts. There is no separate "credit card payment" category, a deliberate
  simplification of the usual envelope treatment.
- **Transfers** are two linked transactions, one per account; editing one
  updates the other. Only the budget side of a budget ↔ tracking transfer
  has a category.
- **Bank sync** is simulated: no networking. Sync refreshes the timestamp and
  clears a connection problem; the sample's last few days of bank
  transactions are "imported" and unapproved.
- **Sample data** (SampleData.swift): the invented Alder household (two
  members and a pending invitation), six accounts (checking, savings, cash,
  credit card; mortgage and retirement as tracking), 15 categories in Bills,
  Everyday and Savings Goals, about a hundred transactions from 1 September to 18
  November 2026 with deterministic amounts (a fixed multiplier table, no
  random numbers). Assignments are computed so earlier months end with
  nothing to assign and November has $412.37 left, with Dining Out
  overspent and Summer Vacation and Holiday Gifts underfunded.

## What is unverified

Nothing here has been compiled or run: the environment was Linux without
Xcode, and no Swift toolchain was installed for a Foundation-only check (the
download was too large for the machine). The likely trouble spots, in order:

- `.defaultAdaptableTabBarPlacement(_:)` (used for `-columns all`) — I believe
  it is iOS 18 / macOS 15 API; if not, delete that one line and the iPad
  sidebar has to be opened by hand.
- `if` inside a `Table`'s column builder (the Account column is omitted in a
  register). If the builder rejects it, always include the column.
- `TabSection` + `ForEach` + `.defaultVisibility(.hidden, for: .tabBar)` for
  per-account sidebar items, and their behaviour on iPhone.
- `contextMenu(forSelectionType:menu:primaryAction:)` triggering on tap in
  compact width on iOS (it is documented to); if it doesn't, iPhone rows need
  a `NavigationLink`.
- `@Entry` for `FocusedValues` and `EnvironmentValues`; `searchFocused`;
  `sign(strategy: .always())` and `.notation(.compactName)` on currency
  formats; `Chart` with tuple data and `\.name` key paths.
- Observation tracking in `Commands` (enabling/disabling menu items as the
  selection changes).
- The arithmetic in BudgetStore and the sample data have not been executed,
  so the stated figures ($412.37 left, which categories are overspent or
  underfunded) are by construction, not observed.
- Layout and appearance: nothing has been seen on a device or simulator.
  Column widths, the Ready to Assign card inside a list section on Mac, and
  the inspector widths are educated guesses.
- The app icon set has no images.
