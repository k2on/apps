// The sheets, each at the height its work needs (Shell.swift gives the
// detents): a transaction typed or corrected, as Wallet's and Reminders'
// forms are laid out, and one already there made a transfer from its
// category's list (Transfer To); an amount assigned, a target set or a balance
// reconciled on the keypad Apple Cash takes an amount on (ArkUI's
// `AmountEntry`); money moved between categories, its amount first and then
// where; transactions given a category from the budget's own list; an
// account added; a category added. Every one closes itself once the domain
// has taken what it said, and stays open with the domain's sentence when it
// refused it.
import ArkUI
import SwiftUI

// MARK: the transaction form

struct TransactionSheet: View {
    @State var draft: Draft
    @EnvironmentObject private var model: Model
    @Environment(\.dismiss) private var dismiss
    @FocusState private var payeeFocused: Bool
    @State private var saved = 0
    @State private var deleting = false

    var body: some View {
        let s = model.snapshot
        NavigationStack {
            Form {
                Section {
                    Picker("", selection: $draft.inflow) {
                        Text("Outflow").tag(false)
                        Text("Inflow").tag(true)
                    }
                    .pickerStyle(.segmented)
                    .listRowBackground(Color.clear)
                    .listRowInsets(EdgeInsets())
                    AmountEntry(cents: $draft.cents, prompt: draft.inflow ? "Coming in" : "Going out",
                                tint: draft.inflow ? Color.green : Color.primary)
                        .padding(.vertical, Space.l)
                        .listRowBackground(Color.clear)
                }
                Section {
                    // Wrapped, never cut: a long payee is read whole. Return
                    // ends it, as it would a one-line field.
                    TextField("Payee", text: $draft.payee, axis: .vertical)
                        .lineLimit(1...4)
                        .focused($payeeFocused)
                        .textInputAutocapitalization(.words)
                        .submitLabel(.done)
                        .disabled(draft.transferTo != nil)
                        .onChange(of: draft.payee) { _, p in
                            if p.contains("\n") {
                                draft.payee = p.replacingOccurrences(of: "\n", with: "")
                                payeeFocused = false
                            }
                        }
                    if payeeFocused {
                        ForEach(Payees.suggest(draft.payee, s.payees)) { p in
                            Button {
                                draft.choose(p)
                                payeeFocused = false
                            } label: {
                                HStack {
                                    Image(systemName: "clock.arrow.circlepath").foregroundStyle(.secondary)
                                    Text(p.name)
                                    Spacer()
                                    if let c = s.line(p.categoryId) { Text(c.name).font(.caption).foregroundStyle(.secondary) }
                                }
                            }
                            .foregroundStyle(.primary)
                        }
                    }
                    // What the bank called it, under the payee as Wallet
                    // shows a merchant's statement name: whole, and copyable.
                    if let bank = draft.bank {
                        VStack(alignment: .leading, spacing: 2) {
                            Text("Bank Description").font(.caption).foregroundStyle(.secondary)
                            Text(bank)
                                .foregroundStyle(.secondary)
                                .fixedSize(horizontal: false, vertical: true)
                                .textSelection(.enabled)
                        }
                        .accessibilityElement(children: .combine)
                    }
                    NavigationLink {
                        // A transaction already there is made a transfer here
                        // too (the domain pairs it with the matching one in the
                        // other account, or adds its other half); a transfer
                        // stays one, and only its other account is chosen.
                        CategoryPicker(chosen: draft.transferTo == nil ? draft.category : nil,
                                       transfers: s.accounts.filter { !$0.closed && $0.id != draft.account },
                                       categories: !draft.transfer, transferred: draft.transferTo,
                                       transferNote: draft.editing != nil && !draft.transfer
                                           ? "Made a transfer, it pairs with the matching transaction in that account, or one is added there." : nil,
                                       inflow: true) { choice in
                            switch choice {
                            case let .category(c): draft.choose(category: c)
                            case let .transfer(a): draft.choose(transfer: a)
                            }
                        }
                    } label: {
                        LabeledContent("Category") { Text(categoryWords(s)).foregroundStyle(draft.category == nil && draft.transferTo == nil ? Color.orange : Color.secondary) }
                    }
                }
                Section {
                    Picker("Account", selection: $draft.account) {
                        ForEach(s.accounts.filter { !$0.closed }) { a in
                            Label(a.name, systemImage: Accounts.symbol(a.kind)).tag(Optional(a.id))
                        }
                    }
                    .disabled(draft.editing != nil)
                    DatePicker("Date", selection: Binding(get: { Days.date(draft.date) ?? Date() }, set: { draft.date = Days.day($0) }),
                               displayedComponents: .date)
                    Toggle(isOn: $draft.cleared) { Label("Cleared", systemImage: "c.circle") }
                        .disabled(draft.editing != nil)
                }
                Section {
                    TextField("Memo", text: $draft.memo, axis: .vertical)
                        .lineLimit(1...4)
                } footer: {
                    if !draft.by.isEmpty { Text("Entered by \(draft.by)") }
                }
                if draft.editing != nil {
                    Section {
                        Button("Delete Transaction", role: .destructive) { deleting = true }
                            .frame(maxWidth: .infinity)
                    }
                }
            }
            .navigationTitle(draft.editing == nil ? "New Transaction" : "Transaction")
            .navigationBarTitleDisplayMode(.inline)
            .toolbar {
                ToolbarItem(placement: .cancellationAction) {
                    Button(role: .cancel) { dismiss() } label: { Image(systemName: "xmark") }
                        .accessibilityLabel("Cancel")
                }
                ToolbarItem(placement: .confirmationAction) {
                    Button(role: .confirm) {
                        model.save(draft) { ok in
                            if ok { saved += 1; dismiss() }
                        }
                    } label: { Image(systemName: "checkmark") }
                        .accessibilityLabel("Save")
                        .disabled(draft.problem != nil)
                }
            }
            .confirmationDialog("Delete this transaction?", isPresented: $deleting, titleVisibility: .visible) {
                Button("Delete Transaction", role: .destructive) {
                    if let id = draft.editing { model.delete([id]) }
                    dismiss()
                }
            }
            .sensoryFeedback(.success, trigger: saved)
        }
    }

    private func categoryWords(_ s: Snapshot) -> String {
        if let t = draft.transferTo { return "Transfer: \(s.account(t)?.name ?? "")" }
        guard let c = draft.category else { return "Choose" }
        if c == Fixed.readyToAssign { return "Ready to Assign" }
        return s.line(c)?.name ?? "Choose"
    }
}

/// What the category list was chosen for.
enum Choice {
    /// A category, Ready to Assign's id for an inflow, or none.
    case category(Id?)
    case transfer(Id)
}

/// The budget's categories under their groups, each with what it has, to
/// choose one from: Ready to Assign first when money can come in, and the
/// other accounts when a transfer can go to one. Searchable, as a long
/// list in a system sheet is. Never a card's payment category: money goes
/// there by a transfer to the card (`Cards.choosable`).
struct CategoryPicker: View {
    let chosen: Id?
    var transfers: [AccountLine] = []
    /// Whether categories are offered at all: not for a transfer, which stays one.
    var categories = true
    /// The account a transfer is with now, marked.
    var transferred: Id? = nil
    /// What choosing an account does, said under them.
    var transferNote: String? = nil
    var inflow = false
    var allowNone = false
    var skip: Id? = nil
    let pick: (Choice) -> Void
    @EnvironmentObject private var model: Model
    @Environment(\.dismiss) private var dismiss
    @State private var needle = ""

    var body: some View {
        let s = model.snapshot
        let n = needle.lowercased()
        List {
            if categories && inflow && (n.isEmpty || "ready to assign".contains(n)) {
                Section("Inflow") {
                    row("Ready to Assign", s.summary?.readyToAssign, chosen == Fixed.readyToAssign) { pick(.category(Fixed.readyToAssign)) }
                }
            }
            ForEach(categories ? Cards.choosable(Months.groups(s.groups, s.lines, hidden: false)) : []) { g in
                let lines = g.lines.filter { $0.categoryId != skip && (n.isEmpty || $0.name.lowercased().contains(n)) }
                if !lines.isEmpty {
                    Section(g.group.name) {
                        ForEach(lines) { l in
                            row(l.name, l.available, chosen == l.categoryId) { pick(.category(l.categoryId)) }
                        }
                    }
                }
            }
            let accounts = transfers.filter { n.isEmpty || $0.name.lowercased().contains(n) }
            if !accounts.isEmpty {
                Section {
                    ForEach(accounts) { a in
                        row(a.name, nil, transferred == a.id) { pick(.transfer(a.id)) }
                    }
                } header: {
                    Text("Transfer To")
                } footer: {
                    if let transferNote { Text(transferNote) }
                }
            }
            if categories && allowNone {
                Section { row("No Category", nil, chosen == nil) { pick(.category(nil)) } }
            }
        }
        .searchable(text: $needle, placement: .navigationBarDrawer(displayMode: .always), prompt: "Category")
        .navigationTitle("Category")
        .navigationBarTitleDisplayMode(.inline)
    }

    private func row(_ name: String, _ cents: Int64?, _ on: Bool, _ then: @escaping () -> Void) -> some View {
        Button {
            then()
            dismiss()
        } label: {
            HStack {
                Text(name).foregroundStyle(.primary)
                Spacer()
                if let c = cents { CentsText(c).foregroundStyle(c < 0 ? Color.red : Color.secondary) }
                if on { Image(systemName: "checkmark").foregroundStyle(Color.money).fontWeight(.semibold) }
            }
        }
        .accessibilityAddTraits(on ? .isSelected : [])
    }
}

/// Transactions given a category, from a sheet of its own; approved with it, from the inbox.
struct CategorizeSheet: View {
    let ids: [Id]
    let approve: Bool
    @EnvironmentObject private var model: Model

    var body: some View {
        NavigationStack {
            CategoryPicker(chosen: nil, inflow: true, allowNone: !approve) { choice in
                guard case let .category(c) = choice else { return }
                model.categorize(ids, c)
            }
            .navigationTitle(ids.count == 1 ? "Categorize" : "Categorize \(ids.count)")
        }
    }
}

// MARK: the budget's amounts

/// A category's assigned amount this month, typed on the keypad.
struct AssignSheet: View {
    let id: Id
    @EnvironmentObject private var model: Model
    @Environment(\.dismiss) private var dismiss
    @State private var cents: Int64 = 0
    @State private var started = false

    var body: some View {
        let l = model.snapshot.line(id)
        NavigationStack {
            VStack(spacing: Space.l) {
                AmountEntry(cents: $cents, prompt: "Assigned in \(model.monthName)", tint: Color.money)
                if let l {
                    HStack(spacing: Space.s) {
                        if let cover = Cards.toCover(l) { chip("Covers the Card", cover) }
                        if l.target > 0 { chip("Target", l.target) }
                        if l.assignedLastMonth > 0 { chip("Last Month", l.assignedLastMonth) }
                        if l.averageSpent > 0 { chip("Average Spent", l.averageSpent) }
                    }
                }
                if let s = model.snapshot.summary {
                    Text("\(CentsText.string(s.readyToAssign - (cents - (l?.assigned ?? 0)))) left to assign")
                        .font(.footnote)
                        .foregroundStyle(.secondary)
                        .contentTransition(.numericText())
                }
                Spacer()
            }
            .padding(Space.page)
            .navigationTitle(l?.name ?? "Assign")
            .navigationBarTitleDisplayMode(.inline)
            .toolbar {
                ToolbarItem(placement: .cancellationAction) {
                    Button(role: .cancel) { dismiss() } label: { Image(systemName: "xmark") }.accessibilityLabel("Cancel")
                }
                ToolbarItem(placement: .confirmationAction) {
                    Button(role: .confirm) {
                        model.assign(id, cents) { if $0 { dismiss() } }
                    } label: { Image(systemName: "checkmark") }
                        .accessibilityLabel("Assign")
                }
            }
            .onAppear {
                if !started { cents = l?.assigned ?? 0; started = true }
            }
        }
    }

    private func chip(_ label: String, _ c: Int64) -> some View {
        Button { cents = c } label: {
            VStack(spacing: 2) {
                Text(label).font(.caption2).foregroundStyle(.secondary)
                CentsText(c).font(.caption.weight(.semibold))
            }
        }
        .buttonStyle(.glass)
    }
}

/// Money moved: out of a category into another (or Ready to Assign), into
/// an overspent one from another (Cover Overspending), or from Ready to
/// Assign into one. The amount first, then where.
struct MoveSheet: View {
    /// The category; none is Ready to Assign.
    let id: Id?
    @EnvironmentObject private var model: Model
    @Environment(\.dismiss) private var dismiss
    @State private var cents: Int64 = 0
    @State private var started = false
    @State private var moved = 0

    var body: some View {
        let l = model.snapshot.line(id)
        let covering = (l?.available ?? 0) < 0
        NavigationStack {
            List {
                Section {
                    AmountEntry(cents: $cents, prompt: covering ? "To cover \(l?.name ?? "")" : "From \(l?.name ?? "Ready to Assign")",
                                tint: covering ? Color.red : Color.money)
                        .padding(.vertical, Space.m)
                        .listRowBackground(Color.clear)
                }
                Section(covering ? "Cover It From" : "Move It To") {
                    if id != nil {
                        target("Ready to Assign", model.snapshot.summary?.readyToAssign, nil, l)
                    }
                    ForEach(Months.groups(model.snapshot.groups, model.snapshot.lines, hidden: false)) { g in
                        ForEach(g.lines.filter { $0.categoryId != id }) { o in
                            target(o.name, o.available, o.categoryId, l)
                        }
                    }
                }
            }
            .navigationTitle(covering ? "Cover Overspending" : id == nil ? "Assign" : "Move Money")
            .navigationBarTitleDisplayMode(.inline)
            .toolbar {
                ToolbarItem(placement: .cancellationAction) {
                    Button(role: .cancel) { dismiss() } label: { Image(systemName: "xmark") }.accessibilityLabel("Cancel")
                }
            }
            .onAppear {
                if !started {
                    cents = id == nil ? max(model.snapshot.summary?.readyToAssign ?? 0, 0) : Moves.suggested(l)
                    started = true
                }
            }
            .sensoryFeedback(.success, trigger: moved)
        }
    }

    private func target(_ name: String, _ has: Int64?, _ other: Id?, _ l: BudgetLine?) -> some View {
        Button {
            model.move(l, other, cents) { ok in
                if ok { moved += 1; dismiss() }
            }
        } label: {
            HStack {
                Text(name).foregroundStyle(.primary)
                Spacer()
                if let c = has { CentsText(c).foregroundStyle(c < 0 ? Color.red : Color.secondary) }
            }
        }
        .disabled(cents <= 0)
    }
}

/// A category's monthly target, typed on the keypad; nothing takes it away.
struct TargetSheet: View {
    let id: Id
    @EnvironmentObject private var model: Model
    @Environment(\.dismiss) private var dismiss
    @State private var cents: Int64 = 0
    @State private var started = false

    var body: some View {
        let l = model.snapshot.line(id)
        NavigationStack {
            VStack(spacing: Space.l) {
                AmountEntry(cents: $cents, prompt: "Set aside each month", tint: Color.money)
                if let l, l.averageSpent > 0 {
                    Button("Use the average spent, \(CentsText.string(l.averageSpent))") { cents = l.averageSpent }
                        .buttonStyle(.glass)
                }
                if let l, l.target > 0 {
                    Button("Remove Target", role: .destructive) {
                        model.setTarget(l.categoryId, 0)
                        dismiss()
                    }
                }
                Spacer()
            }
            .padding(Space.page)
            .navigationTitle(l.map { "\($0.name) Target" } ?? "Target")
            .navigationBarTitleDisplayMode(.inline)
            .toolbar {
                ToolbarItem(placement: .cancellationAction) {
                    Button(role: .cancel) { dismiss() } label: { Image(systemName: "xmark") }.accessibilityLabel("Cancel")
                }
                ToolbarItem(placement: .confirmationAction) {
                    Button(role: .confirm) {
                        model.setTarget(id, cents)
                        dismiss()
                    } label: { Image(systemName: "checkmark") }
                        .accessibilityLabel("Save Target")
                }
            }
            .onAppear {
                if !started { cents = l?.target ?? 0; started = true }
            }
        }
    }
}

// MARK: accounts

struct AddAccountSheet: View {
    @EnvironmentObject private var model: Model
    @Environment(\.dismiss) private var dismiss
    @State private var name = ""
    @State private var kind = "checking"
    @State private var onBudget = true
    @State private var cents: Int64 = 0
    @State private var negative = false
    @State private var why: String? = nil

    var body: some View {
        NavigationStack {
            Form {
                Section {
                    TextField("Name", text: $name)
                        .textInputAutocapitalization(.words)
                    Picker("Type", selection: $kind) {
                        ForEach(Accounts.kinds, id: \.kind) { k in
                            Label(k.name, systemImage: Accounts.symbol(k.kind)).tag(k.kind)
                        }
                    }
                    .onChange(of: kind) { _, k in onBudget = Accounts.onBudget(byDefault: k) }
                }
                Section {
                    AmountEntry(cents: $cents, prompt: Accounts.owes(kind) ? "Owed on it today" : "In it today")
                        .padding(.vertical, Space.m)
                } header: {
                    Text(Accounts.owes(kind) ? "Balance Owed" : "Balance")
                }
                Section {
                    Toggle("In the Budget", isOn: $onBudget)
                } footer: {
                    Text(onBudget ? "What's spent from it comes out of the budget, and its balance is ready to assign."
                                  : "Tracking only: its balance counts toward net worth, and none of it is budgeted.")
                }
                if let why {
                    Section { Text(why).foregroundStyle(Color.red) }
                }
            }
            .navigationTitle("Add Account")
            .navigationBarTitleDisplayMode(.inline)
            .toolbar {
                ToolbarItem(placement: .cancellationAction) {
                    Button(role: .cancel) { dismiss() } label: { Image(systemName: "xmark") }.accessibilityLabel("Cancel")
                }
                ToolbarItem(placement: .confirmationAction) {
                    Button(role: .confirm) {
                        let balance = Accounts.owes(kind) ? -cents : cents
                        model.openAccount(name: name.trimmingCharacters(in: .whitespaces), kind: kind, onBudget: onBudget, balance: balance) { ok in
                            if ok { dismiss() } else { why = model.notice }
                        }
                    } label: { Image(systemName: "checkmark") }
                        .accessibilityLabel("Add Account")
                        .disabled(name.trimmingCharacters(in: .whitespaces).isEmpty)
                }
            }
        }
    }
}

/// Agreeing with the bank: is the cleared balance what the bank says? Yes
/// locks what was cleared; no asks what the bank says, and the difference
/// is entered as an adjustment.
struct ReconcileSheet: View {
    let id: Id
    @EnvironmentObject private var model: Model
    @Environment(\.dismiss) private var dismiss
    @State private var differs = false
    @State private var cents: Int64 = 0
    @State private var done = 0

    var body: some View {
        let a = model.snapshot.account(id)
        let owes = Accounts.owes(a?.kind ?? "")
        let cleared = a?.cleared ?? 0
        NavigationStack {
            VStack(spacing: Space.l) {
                if !differs {
                    Text("Is your cleared balance")
                        .font(.title3)
                    CentsText(cleared).font(.system(size: 44, weight: .bold, design: .rounded))
                    if let b = a?.bankBalance {
                        Label("The bank last said \(CentsText.string(b))", systemImage: "building.columns")
                            .font(.footnote)
                            .foregroundStyle(b == cleared ? Color.green : Color.orange)
                    }
                    HStack(spacing: Space.m) {
                        Button("No") {
                            cents = Int64((a?.bankBalance ?? cleared).magnitude)
                            withAnimation(Springs.move) { differs = true }
                        }
                        .buttonStyle(.glass)
                        .controlSize(.large)
                        Button("Yes") { finish(cleared) }
                            .buttonStyle(.glassProminent)
                            .tint(Color.money)
                            .controlSize(.large)
                    }
                    .padding(.top, Space.m)
                } else {
                    AmountEntry(cents: $cents, prompt: "What the bank says it is" + (owes ? ", owed" : ""), tint: Color.money)
                    Text("The difference is entered as an adjustment, and everything cleared is locked.")
                        .font(.footnote)
                        .foregroundStyle(.secondary)
                        .multilineTextAlignment(.center)
                    Button("Reconcile") { finish(owes ? -cents : cents) }
                        .buttonStyle(.glassProminent)
                        .tint(Color.money)
                        .controlSize(.large)
                }
                Spacer()
            }
            .padding(Space.page)
            .navigationTitle("Reconcile \(a?.name ?? "")")
            .navigationBarTitleDisplayMode(.inline)
            .toolbar {
                ToolbarItem(placement: .cancellationAction) {
                    Button(role: .cancel) { dismiss() } label: { Image(systemName: "xmark") }.accessibilityLabel("Cancel")
                }
            }
            .sensoryFeedback(.success, trigger: done)
        }
    }

    private func finish(_ balance: Int64) {
        model.reconcile(id, balance: balance) { ok in
            if ok { done += 1; dismiss() }
        }
    }
}

// MARK: categories

/// A category added to a group, or a group chosen first when none was.
struct NewCategorySheet: View {
    let group: Id?
    @EnvironmentObject private var model: Model
    @Environment(\.dismiss) private var dismiss
    @State private var name = ""
    @State private var chosen: Id? = nil
    @FocusState private var focused: Bool

    var body: some View {
        NavigationStack {
            Form {
                TextField("Name", text: $name)
                    .focused($focused)
                    .textInputAutocapitalization(.words)
                    .submitLabel(.done)
                    .onSubmit(add)
                Picker("Group", selection: $chosen) {
                    // Not the cards' group: its categories come with the cards.
                    ForEach(model.snapshot.groups.filter { !Cards.isGroup($0.id) }) { g in Text(g.name).tag(Optional(g.id)) }
                }
            }
            .navigationTitle("New Category")
            .navigationBarTitleDisplayMode(.inline)
            .toolbar {
                ToolbarItem(placement: .cancellationAction) {
                    Button(role: .cancel) { dismiss() } label: { Image(systemName: "xmark") }.accessibilityLabel("Cancel")
                }
                ToolbarItem(placement: .confirmationAction) {
                    Button(role: .confirm, action: add) { Image(systemName: "checkmark") }
                        .accessibilityLabel("Add Category")
                        .disabled(name.trimmingCharacters(in: .whitespaces).isEmpty || chosen == nil)
                }
            }
            .onAppear {
                chosen = group ?? model.snapshot.groups.first { !Cards.isGroup($0.id) }?.id
                focused = true
            }
        }
    }

    private func add() {
        guard let g = chosen else { return }
        let n = name.trimmingCharacters(in: .whitespaces)
        guard !n.isEmpty else { return }
        model.addCategory(g, n)
        dismiss()
    }
}
