// The Budget tab: the month's plan, as the desktop's budget page has it
// (Ready to Assign across the top, the groups' categories with what each
// was assigned, spent and has), drawn the way Apple's own apps would. The
// month is paged under the large title; Ready to Assign is the screen's one
// large figure, on glass, as Apple Card's balance is, and tapping it is
// assigning it; each category is a row with its available amount in a pill
// of its tone and, when it has a target, how far the month is toward it.
// A row opens the category's month (CategoryView: target, move money, cover
// overspending); swiping it assigns or moves money, and holding it offers
// everything else. Pull to refresh asks the bank for anything new.
//
// The cards' group, Credit Card Payments, comes first, as YNAB draws it:
// each card's payment category wears the card's mark and says how much of
// what the card owes is set aside — a bar, "$X short" or "Covered" — and
// the group what of the cards' debt is not covered. A category that spent
// more on a card than it had is amber, "$X on card": debt, not the cash
// overspending that is red. The cards' group and its categories are
// named, filled and emptied as the cards are, so they offer no rename,
// target or delete.
//
// A category whose month follows a rule (set on the desktop) wears the
// rule's mark by what it was assigned, and offers no assigning or moving:
// its swipes are gone and holding it says the rule.
//
// The large title's line has the ••• menu and the account's face, no +: a
// new transaction is the menu's first item (and the Transactions tab's +),
// a new category or group the menu's and each group's own Add a Category.
//
// Reorder Categories (the ••• menu) is the list's Edit mode, as Reminders
// has its lists and their groups: the budget becomes one list, each group a
// row over its categories, every row with the system's grabber. A category
// dragged under another group joins it there; a group dragged takes its
// categories with it. What was let go is drawn as it will be (`ahead`)
// until the session's answer says the same, so nothing jumps back.
import ArkUI
import SwiftUI

struct BudgetView: View {
    @EnvironmentObject private var model: Model
    @Environment(\.reducesMotion) private var still
    @Environment(\.present) private var present
    @AppStorage("budget.hidden") private var showHidden = false
    @State private var collapsed: Set<Id> = []
    @State private var naming: Naming? = nil
    @State private var typed = ""
    @State private var editing = false
    /// The groups as a drop left them, drawn ahead of the session's saying so.
    @State private var ahead: [GroupMonth]? = nil
    @State private var drops = 0

    /// A name being typed in an alert: a new group, or a group renamed.
    enum Naming: Identifiable {
        case newGroup
        case group(GroupLine)
        var id: String { if case let .group(g) = self { return g.id.hex } else { return "new" } }
        var title: String { if case .newGroup = self { return "New Group" } else { return "Rename Group" } }
        var verb: String { if case .newGroup = self { return "Add" } else { return "Rename" } }
    }


    var body: some View {
        let s = model.snapshot
        let groups = Months.groups(s.groups, s.lines, hidden: showHidden)
        let shown = ahead ?? groups
        List {
            // The month is the toolbar's title, its arrows the toolbar's
            // (the shell's); Ready to Assign is the page's headline.
            Section { ready(s.summary) }
            if !s.loaded {
                Section { HStack { Spacer(); ProgressView(); Spacer() } }
                    .listRowBackground(Color.clear)
            }
            if editing {
                Section {
                    ForEach(Arranging.rows(shown)) { r in arranged(r) }
                        .onMove { from, to in drop(shown, from, to) }
                } footer: {
                    Text("Drag a category under another group to move it there. A group moves with its categories; a card's payment stays with the cards.")
                }
            } else {
                ForEach(shown) { g in
                    group(g)
                }
            }
        }
        .editing(editing)
        .onChange(of: Arranging.shape(groups)) { _, now in
            if let a = ahead, Arranging.shape(a) == now { ahead = nil }
        }
        .groupedList()
        .compactSections()
        .refreshable { await model.refresh() }
        .toolbar {
            ToolbarItem(placement: .secondaryAction) {
                if editing {
                    Button("Done") { withAnimation(Springs.move.reduced(still)) { editing = false } }
                        .fontWeight(.semibold)
                } else {
                    options
                }
            }
        }
        .alert(naming?.title ?? "",
               isPresented: Binding(get: { naming != nil }, set: { if !$0 { naming = nil } })) {
            TextField("Name", text: $typed)
            Button("Cancel", role: .cancel) { naming = nil }
            Button(naming?.verb ?? "Save") {
                let name = typed.trimmingCharacters(in: .whitespaces)
                switch naming {
                case .newGroup?: model.addGroup(name)
                case let .group(g)?: model.rename(group: g.id, name)
                case nil: break
                }
                naming = nil
            }
        }
        .sensoryFeedback(.selection, trigger: model.month)
    }

    private var options: some View {
        Menu {
            Section {
                Button {
                    present(.transaction(TxnForm(account: nil, accounts: model.snapshot.accounts, date: model.today)))
                } label: { Label("Add Transaction", systemImage: "plus") }
            }
            Section("Auto-Assign") {
                Button { model.autoAssign(.underfunded) } label: { Label("Underfunded", systemImage: "target") }
                Button { model.autoAssign(.lastMonth) } label: { Label("Assigned Last Month", systemImage: "arrow.uturn.backward") }
                Button(role: .destructive) { model.autoAssign(.reset) } label: { Label("Reset Assigned Amounts", systemImage: "arrow.counterclockwise") }
            }
            Section {
                Button { present(.newCategory(group: nil)) } label: { Label("New Category", systemImage: "plus.circle") }
                Button { typed = ""; naming = .newGroup } label: { Label("New Group", systemImage: "folder.badge.plus") }
                Button { withAnimation(Springs.move.reduced(still)) { editing = true } } label: { Label("Reorder Categories", systemImage: "arrow.up.arrow.down") }
                Toggle(isOn: $showHidden) { Label("Show Hidden Categories", systemImage: "eye") }
            }
        } label: {
            Image(systemName: "ellipsis")
        }
        .accessibilityLabel("Budget options")
    }

    // MARK: Edit

    /// A row of Edit: a group as Reminders draws one, its categories under it.
    @ViewBuilder private func arranged(_ r: Arranged) -> some View {
        switch r {
        case let .group(g):
            HStack(spacing: Space.m) {
                Image(systemName: "folder.fill").foregroundStyle(.tint)
                Text(g.name).font(.headline).lineLimit(1)
                if g.hidden { Image(systemName: "eye.slash").font(.caption).foregroundStyle(.quiet) }
            }
            .accessibilityElement(children: .combine)
            .accessibilityLabel("\(g.name) group")
        case let .category(l):
            HStack {
                if l.isPayment() { AccountMark(kind: "credit", side: 22) }
                Text(l.name)
                    .foregroundStyle(l.hidden ? Color.quiet : Color.ink)
                    .lineLimit(1)
                Spacer(minLength: Space.s)
                Pill(cents: l.available, tint: l.tone().tint).rolls(on: model.snapshot.month)
            }
            .padding(.leading, Space.xl + Space.s)
            .accessibilityElement(children: .ignore)
            .accessibilityLabel(l.name)
            .accessibilityValue("\(CentsText.string(l.available)) available")
        }
    }

    /// A row let go: drawn where it went and asked of the budget, and let
    /// go of once the session agrees, or after a while if it never does.
    private func drop(_ groups: [GroupMonth], _ from: IndexSet, _ to: Int) {
        guard let f = from.first, let r = Arranging.drop(Arranging.rows(groups), from: f, to: to) else { return }
        withAnimation(Springs.move.reduced(still)) { ahead = Arranging.applied(r, to: groups) }
        model.rearrange(r)
        drops += 1
        let mine = drops
        Task { @MainActor in
            try? await Task.sleep(for: .seconds(2))
            if drops == mine { withAnimation(Springs.move.reduced(still)) { ahead = nil } }
        }
    }

    // MARK: Ready to Assign

    /// What needs attention, in words: so many overspent, so many
    /// underfunded; or, with nothing, what the money is for.
    private static func caption(_ s: Summary) -> String {
        var said: [String] = []
        if s.overspent > 0 { said.append("\(s.overspent) overspent") }
        if s.underfunded > 0 { said.append("\(s.underfunded) underfunded") }
        return said.isEmpty ? (s.readyToAssign == 0 ? "Every dollar has a job." : "Give these dollars a job.") : said.joined(separator: " \u{b7} ")
    }

    @ViewBuilder private func ready(_ summary: Summary?) -> some View {
        let s = summary ?? Summary(month: model.month, readyToAssign: 0, income: 0, assigned: 0, activity: 0, available: 0, overspentLastMonth: 0, overspent: 0, underfunded: 0)
        let caption = Self.caption(s)
        Headline(s.readyToAssign < 0 ? "Assigned Too Much" : "Ready to Assign",
                 figure: CentsText.string(s.readyToAssign),
                 tint: s.readyToAssign < 0 ? .negative : s.readyToAssign > 0 ? .positive : .neutral,
                 caption: caption) {
            Menu {
                Button { model.autoAssign(.underfunded) } label: { Label("Underfunded", systemImage: "target") }
                Button { model.autoAssign(.lastMonth) } label: { Label("Assigned Last Month", systemImage: "arrow.uturn.backward") }
                Divider()
                Button(role: .destructive) { model.autoAssign(.reset) } label: { Label("Reset Assigned Amounts", systemImage: "arrow.counterclockwise") }
            } label: {
                Label("Auto-Assign", systemImage: "wand.and.stars")
            }
            .buttonStyle(.borderedProminent)
            .controlSize(.small)
            .fixedSize()
        }
    }

    // MARK: a group

    @ViewBuilder private func group(_ g: GroupMonth) -> some View {
        let open = !collapsed.contains(g.id)
        Section {
            if open {
                ForEach(g.lines) { l in
                    // A month its rule assigns offers neither: the domain
                    // refuses assigning it and moving money to or from it.
                    NavigationLink(value: Route.category(id: l.categoryId.hex, name: l.name)) { CategoryRow(line: l, month: model.snapshot.month) }
                        .swipeActions(edge: .leading, allowsFullSwipe: true) {
                            if l.assignable() {
                                Button { present(.assign(l.categoryId)) } label: { Label("Assign", systemImage: "plus.circle.fill") }
                                    .tint(Color.accent)
                            }
                        }
                        .swipeActions(edge: .trailing) {
                            if l.assignable() {
                                Button { present(.move(l.categoryId)) } label: {
                                    Label(l.available < 0 ? "Cover" : "Move", systemImage: "arrow.left.arrow.right")
                                }
                                .tint(l.available < 0 ? l.tone().tint.color : Color.chart[1])
                            }
                        }
                        .contextMenu { menu(l) }
                }
                if g.lines.isEmpty && !g.isCards {
                    Button { present(.newCategory(group: g.id)) } label: { Label("Add a Category", systemImage: "plus") }
                }
            }
        } header: {
            Button {
                withAnimation(Springs.move.reduced(still)) {
                    if open { collapsed.insert(g.id) } else { collapsed.remove(g.id) }
                }
            } label: {
                HStack {
                    Image(systemName: "chevron.right")
                        .font(.caption.weight(.bold))
                        .rotationEffect(.degrees(open ? 90 : 0))
                    Text(g.group.name)
                    if g.group.hidden { Image(systemName: "eye.slash").font(.caption) }
                    Spacer()
                    // The cards' group says what of their debt is not
                    // covered; any other, what its categories have.
                    if g.isCards && g.uncovered > 0 {
                        Text("\(CentsText.string(g.uncovered)) not covered")
                            .font(.subheadline.weight(.semibold))
                            .monospacedDigit()
                            .foregroundStyle(Color.caution)
                    } else {
                        CentsText(g.available).font(.subheadline.weight(.semibold)).rolls(on: model.snapshot.month)
                            .foregroundStyle(g.available < 0 ? Color.error : Color.quiet)
                    }
                }
                .contentShape(Rectangle())
            }
            .buttonStyle(.plain)
            .accessibilityLabel("\(g.group.name), \(CentsText.string(g.available)) available"
                + (g.uncovered > 0 ? ", \(CentsText.string(g.uncovered)) of the cards' debt not covered" : ""))
            .accessibilityHint(open ? "Collapses the group" : "Expands the group")
            .contextMenu {
                // The cards' group is the cards': hidden or shown, but
                // named, filled and emptied only as the cards are.
                if !g.isCards {
                    Button { present(.newCategory(group: g.id)) } label: { Label("New Category", systemImage: "plus.circle") }
                    Button { typed = g.group.name; naming = .group(g.group) } label: { Label("Rename", systemImage: "pencil") }
                }
                Button { model.hide(group: g.id, !g.group.hidden) } label: {
                    Label(g.group.hidden ? "Show" : "Hide", systemImage: g.group.hidden ? "eye" : "eye.slash")
                }
                if !g.isCards {
                    Button(role: .destructive) { model.delete(group: g.id, moveTo: nil) } label: { Label("Delete Group", systemImage: "trash") }
                }
            }
        }
    }

    @ViewBuilder private func menu(_ l: BudgetLine) -> some View {
        // A card's payment: covering its debt first; no target, and no
        // deleting a category that goes when its card does.
        // A month its rule assigns: the rule said, and nothing to assign or move.
        let card = l.isPayment()
        if let words = l.ruleWords() {
            Label(words, systemImage: Ruled.symbol)
        } else {
            if let cover = l.toCover() {
                Button { model.assign(l.categoryId, cover) } label: {
                    Label("Assign \(CentsText.string(l.underfunded)) to Cover the Card", systemImage: "creditcard")
                }
            }
            Button { present(.assign(l.categoryId)) } label: { Label("Assign…", systemImage: "plus.circle") }
            if !card && l.target > l.assigned {
                Button { model.assign(l.categoryId, l.target) } label: {
                    Label("Assign \(CentsText.string(l.toTarget())) to Meet Target", systemImage: "target")
                }
            }
            Button { present(.move(l.categoryId)) } label: {
                Label(l.available < 0 ? "Cover Overspending…" : "Move Money…", systemImage: "arrow.left.arrow.right")
            }
        }
        if !card {
            Button { present(.target(l.categoryId)) } label: { Label(l.target > 0 ? "Edit Target…" : "Set Target…", systemImage: "scope") }
        }
        Divider()
        Button { model.hide(category: l.categoryId, !l.hidden) } label: {
            Label(l.hidden ? "Show" : "Hide", systemImage: l.hidden ? "eye" : "eye.slash")
        }
        if !card {
            Button(role: .destructive) { model.delete(category: l.categoryId, moveTo: nil) } label: { Label("Delete Category", systemImage: "trash") }
        }
    }
}

/// One category in the month: its name, the slim bar toward its goal and
/// what needs attention in words under it; on a regular width what it was
/// given and what was spent in their columns; what it has in a capsule of
/// its state (`StatusBadge`), the capsule alone when compact. A card's
/// payment category wears the card's mark and says how much of what the
/// card owes is set aside.
struct CategoryRow: View {
    let line: BudgetLine
    /// The month its figures are of: they roll when it is stepped.
    let month: String
    @Environment(\.horizontalSizeClass) private var size
    @Environment(\.dynamicTypeSize) private var type
    @ScaledMetric(relativeTo: .body) private var column: CGFloat = LayoutTokens.amountColumnWidth

    var body: some View {
        let tone = line.tone()
        let card = line.isPayment()
        let columns = size != .compact && !type.isAccessibilitySize
        HStack(spacing: Space.m) {
            VStack(alignment: .leading, spacing: Space.xs) {
                HStack(spacing: Space.s) {
                    if card { AccountMark(kind: "credit", side: 20) }
                    Text(line.name)
                        .foregroundStyle(line.hidden ? Color.quiet : Color.ink)
                        .lineLimit(2)
                }
                if card {
                    payment
                } else if let m = line.meter() {
                    GoalBar(m.fraction(), tint: m.tone.tint)
                }
                if !card && (tone == .overspent || tone == .short || tone == .onCard) {
                    Text(line.story(style: Numbers.style)).font(.caption).foregroundStyle(tone.tint.color)
                }
            }
            Spacer(minLength: Space.s)
            if columns {
                CentsText(line.assigned).frame(width: column, alignment: .trailing)
                CentsText(line.activity).foregroundStyle(.quiet).frame(width: column, alignment: .trailing)
                StatusBadge(CentsText.string(line.available), tint: tone.tint)
                    .rolls(on: month)
                    .frame(width: column, alignment: .trailing)
            } else {
                StatusBadge(CentsText.string(line.available), tint: tone.tint).rolls(on: month)
            }
        }
        .padding(.vertical, 2)
        .accessibilityElement(children: .ignore)
        .accessibilityLabel(line.name)
        .accessibilityValue(card ? paymentWords : words(tone))
        .accessibilityHint(line.assignable() ? "Swipe right to assign, left to move money" : "Its rule assigns it")
    }

    /// A card's payment under its name: how much of the debt is set aside,
    /// as a bar, and whether that covers it.
    @ViewBuilder private var payment: some View {
        if let c = line.meter()?.fraction() {
            ProgressView(value: c)
                .tint(c >= 1 ? Color.positive : Color.caution)
                .scaleEffect(y: 0.8, anchor: .center)
            // The core's two ends of the line: covered or short, and what the card owes.
            let words = line.captions(style: Numbers.style)
            HStack {
                Text(words.left).foregroundStyle(line.underfunded > 0 ? Color.caution : Color.positive)
                Spacer()
                Text(words.right).foregroundStyle(.quiet)
            }
            .font(.caption)
            .monospacedDigit()
        } else {
            Text("Nothing owed").font(.caption).foregroundStyle(.quiet)
        }
    }

    private func words(_ tone: Tone) -> String {
        var w = "\(CentsText.string(line.available)) available, \(tone.words()). Assigned \(CentsText.string(line.assigned))"
        if line.target > 0 { w += " of a \(CentsText.string(line.target)) target" }
        if line.ruled { w += ", by its rule" }
        if line.creditOverspent > 0 { w += ". \(CentsText.string(line.creditOverspent)) spent on a card beyond what it had" }
        return w
    }

    private var paymentWords: String {
        "\(CentsText.string(line.available)) set aside for the card, which owes \(CentsText.string(line.debt))"
            + (line.underfunded > 0 ? ". \(CentsText.string(line.underfunded)) not covered" : "")
    }
}
