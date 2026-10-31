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
            Section {
                VStack(spacing: Space.l) {
                    Pager(model.monthName,
                          home: model.month == model.thisMonth ? nil : { withAnimation(Springs.move) { model.showThisMonth() } },
                          back: { withAnimation(Springs.move) { model.stepMonth(-1) } },
                          forward: { withAnimation(Springs.move) { model.stepMonth(1) } })
                    ready(s.summary)
                }
                .listRowInsets(EdgeInsets())
                .listRowBackground(Color.clear)
            }
            if !s.loaded {
                Section { HStack { Spacer(); ProgressView(); Spacer() } }
                    .listRowBackground(Color.clear)
            }
            if editing {
                Section {
                    ForEach(Arranging.rows(shown)) { r in arranged(r) }
                        .onMove { from, to in drop(shown, from, to) }
                } footer: {
                    Text("Drag a category under another group to move it there. A group moves with its categories.")
                }
            } else {
                ForEach(shown) { g in
                    group(g)
                }
            }
        }
        .environment(\.editMode, .constant(editing ? .active : .inactive))
        .onChange(of: Arranging.shape(groups)) { _, now in
            if let a = ahead, Arranging.shape(a) == now { ahead = nil }
        }
        .listStyle(.insetGrouped)
        .listSectionSpacing(.compact)
        .navigationTitle("Budget")
        .refreshable { await model.refresh() }
        .toolbar {
            ToolbarItem(placement: .topBarLeading) { AccountButton() }
            ToolbarItemGroup(placement: .topBarTrailing) {
                if editing {
                    Button("Done") { withAnimation(Springs.move) { editing = false } }
                        .fontWeight(.semibold)
                } else {
                    options
                    AddTransactionButton()
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
            Section("Auto-Assign") {
                Button { model.autoAssign(.underfunded) } label: { Label("Underfunded", systemImage: "target") }
                Button { model.autoAssign(.lastMonth) } label: { Label("Assigned Last Month", systemImage: "arrow.uturn.backward") }
                Button(role: .destructive) { model.autoAssign(.reset) } label: { Label("Reset Assigned Amounts", systemImage: "arrow.counterclockwise") }
            }
            Section {
                Button { present(.newCategory(group: nil)) } label: { Label("New Category", systemImage: "plus.circle") }
                Button { typed = ""; naming = .newGroup } label: { Label("New Group", systemImage: "folder.badge.plus") }
                Button { withAnimation(Springs.move) { editing = true } } label: { Label("Reorder Categories", systemImage: "arrow.up.arrow.down") }
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
                if g.hidden { Image(systemName: "eye.slash").font(.caption).foregroundStyle(.secondary) }
            }
            .accessibilityElement(children: .combine)
            .accessibilityLabel("\(g.name) group")
        case let .category(l):
            HStack {
                Text(l.name)
                    .foregroundStyle(l.hidden ? Color.secondary : Color.primary)
                    .lineLimit(1)
                Spacer(minLength: Space.s)
                Pill(cents: l.available, tint: Tone.of(l).tint)
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
        withAnimation(Springs.move) { ahead = Arranging.applied(r, to: groups) }
        model.rearrange(r)
        drops += 1
        let mine = drops
        Task { @MainActor in
            try? await Task.sleep(for: .seconds(2))
            if drops == mine { withAnimation(Springs.move) { ahead = nil } }
        }
    }

    // MARK: Ready to Assign

    @ViewBuilder private func ready(_ summary: Summary?) -> some View {
        let s = summary ?? Summary(month: model.month, readyToAssign: 0, income: 0, assigned: 0, activity: 0, available: 0, overspentLastMonth: 0, overspent: 0, underfunded: 0)
        HeroFigure("Ready to Assign", cents: s.readyToAssign, tint: s.tint, note: readyWords(s),
                   action: s.readyToAssign > 0 ? { present(.move(nil)) } : nil) {
            HStack(spacing: Space.s) {
                if s.overspent > 0 {
                    Pill(.bad) { Label("\(s.overspent) overspent", systemImage: "exclamationmark.circle.fill") }
                }
                if s.underfunded > 0 {
                    Button { model.autoAssign(.underfunded) } label: {
                        Pill(.caution) { Label("\(s.underfunded) underfunded", systemImage: "target") }
                    }
                    .buttonStyle(.plain)
                    .accessibilityHint("Assigns every underfunded category its target")
                }
                if s.overspent == 0 && s.underfunded == 0 && s.assigned > 0 {
                    Pill(.good) { Label("On track", systemImage: "checkmark.circle.fill") }
                }
                Spacer(minLength: 0)
            }
        }
        .padding(.horizontal, Space.page)
        .padding(.bottom, Space.s)
    }

    // MARK: a group

    @ViewBuilder private func group(_ g: GroupMonth) -> some View {
        let open = !collapsed.contains(g.id)
        Section {
            if open {
                ForEach(g.lines) { l in
                    NavigationLink(value: Route.category(l.categoryId)) { CategoryRow(line: l) }
                        .swipeActions(edge: .leading, allowsFullSwipe: true) {
                            Button { present(.assign(l.categoryId)) } label: { Label("Assign", systemImage: "plus.circle.fill") }
                                .tint(.money)
                        }
                        .swipeActions(edge: .trailing) {
                            Button { present(.move(l.categoryId)) } label: {
                                Label(l.available < 0 ? "Cover" : "Move", systemImage: "arrow.left.arrow.right")
                            }
                            .tint(l.available < 0 ? Color.red : Color.indigo)
                        }
                        .contextMenu { menu(l) }
                }
                if g.lines.isEmpty {
                    Button { present(.newCategory(group: g.id)) } label: { Label("Add a Category", systemImage: "plus") }
                }
            }
        } header: {
            Button {
                withAnimation(Springs.move) {
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
                    CentsText(g.available).font(.subheadline.weight(.semibold))
                        .foregroundStyle(g.available < 0 ? Color.red : Color.secondary)
                }
                .contentShape(Rectangle())
            }
            .buttonStyle(.plain)
            .accessibilityLabel("\(g.group.name), \(CentsText.string(g.available)) available")
            .accessibilityHint(open ? "Collapses the group" : "Expands the group")
            .contextMenu {
                Button { present(.newCategory(group: g.id)) } label: { Label("New Category", systemImage: "plus.circle") }
                Button { typed = g.group.name; naming = .group(g.group) } label: { Label("Rename", systemImage: "pencil") }
                Button { model.hide(group: g.id, !g.group.hidden) } label: {
                    Label(g.group.hidden ? "Show" : "Hide", systemImage: g.group.hidden ? "eye" : "eye.slash")
                }
                Button(role: .destructive) { model.delete(group: g.id, moveTo: nil) } label: { Label("Delete Group", systemImage: "trash") }
            }
        }
    }

    @ViewBuilder private func menu(_ l: BudgetLine) -> some View {
        Button { present(.assign(l.categoryId)) } label: { Label("Assign…", systemImage: "plus.circle") }
        if l.target > l.assigned {
            Button { model.assign(l.categoryId, l.target) } label: {
                Label("Assign \(CentsText.string(Moves.toTarget(l))) to Meet Target", systemImage: "target")
            }
        }
        Button { present(.move(l.categoryId)) } label: {
            Label(l.available < 0 ? "Cover Overspending…" : "Move Money…", systemImage: "arrow.left.arrow.right")
        }
        Button { present(.target(l.categoryId)) } label: { Label(l.target > 0 ? "Edit Target…" : "Set Target…", systemImage: "scope") }
        Divider()
        Button { model.hide(category: l.categoryId, !l.hidden) } label: {
            Label(l.hidden ? "Show" : "Hide", systemImage: l.hidden ? "eye" : "eye.slash")
        }
        Button(role: .destructive) { model.delete(category: l.categoryId, moveTo: nil) } label: { Label("Delete Category", systemImage: "trash") }
    }
}

/// One category in the month: its name, how far it is toward its target,
/// and what it has, in its tone's pill.
struct CategoryRow: View {
    let line: BudgetLine

    var body: some View {
        let tone = Tone.of(line)
        VStack(alignment: .leading, spacing: Space.xs + 2) {
            HStack(alignment: .firstTextBaseline) {
                Text(line.name)
                    .foregroundStyle(line.hidden ? Color.secondary : Color.primary)
                    .lineLimit(1)
                Spacer(minLength: Space.s)
                Pill(cents: line.available, tint: tone.tint)
            }
            if let f = funded(line) {
                ProgressView(value: f)
                    .tint(tone == .over ? Color.red : f >= 1 ? Color.green : Color.orange)
                    .scaleEffect(y: 0.8, anchor: .center)
                HStack {
                    Text("\(CentsText.string(line.assigned)) of \(CentsText.string(line.target))")
                    Spacer()
                    if line.activity != 0 { Text("Spent \(CentsText.string(-line.activity))") }
                }
                .font(.caption)
                .foregroundStyle(.secondary)
                .monospacedDigit()
            } else if line.activity != 0 || line.assigned != 0 {
                HStack {
                    Text("Assigned \(CentsText.string(line.assigned))")
                    Spacer()
                    if line.activity != 0 { Text("Spent \(CentsText.string(-line.activity))") }
                }
                .font(.caption)
                .foregroundStyle(.secondary)
                .monospacedDigit()
            }
        }
        .padding(.vertical, 2)
        .accessibilityElement(children: .ignore)
        .accessibilityLabel(line.name)
        .accessibilityValue("\(CentsText.string(line.available)) available, \(tone.words). Assigned \(CentsText.string(line.assigned))" + (line.target > 0 ? " of a \(CentsText.string(line.target)) target" : ""))
        .accessibilityHint("Swipe right to assign, left to move money")
    }
}
