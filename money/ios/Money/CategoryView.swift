// A category's month: the desktop's inspector, as a page of its own. Its
// available balance large at the top in its tone, how it got there (left
// over from last month, assigned, spent), the one thing to do about it —
// Cover Overspending when it is below nothing, Move Money otherwise — its
// target with a ring of how far the month is toward it and the button that
// meets it, Auto-Assign's three amounts as one-tap choices, the month's
// transactions in it, and managing it (rename, hide, delete) at the end,
// as Settings' rows put the destructive one last. Spending on a card beyond
// what it had is said apart, in amber: debt, not cash gone.
//
// A card's payment category is the desktop's card inspector: what is set
// aside for the card, "Assign $X" at a tap when that falls short of what
// the card owes, the debt and what of it is not covered, and the card's
// month.
import ArkUI
import SwiftUI

struct CategoryView: View {
    let id: Id
    @EnvironmentObject private var model: Model
    @Environment(\.present) private var present
    @Environment(\.dismiss) private var dismiss
    @State private var renaming = false
    @State private var typed = ""
    @State private var deleting = false
    @State private var assigned = 0

    var body: some View {
        if let l = model.snapshot.line(id) {
            if Cards.isPayment(l) { payment(l) } else { page(l) }
        } else {
            ContentUnavailableView("No Category", systemImage: "questionmark.folder", description: Text("It may have been deleted on another device."))
        }
    }

    private func page(_ l: BudgetLine) -> some View {
        let tone = Tone.of(l)
        let spentHere = (model.register(nil) ?? []).filter { $0.categoryId == id && Months.of($0.date) == model.month }
        return List {
            Section {
                HeroFigure("Available", cents: l.available, tint: tone.tint, note: model.monthName) {
                    Button {
                        present(.move(l.categoryId))
                    } label: {
                        Label(l.available < 0 ? "Cover Overspending" : "Move Money", systemImage: "arrow.left.arrow.right")
                            .frame(maxWidth: .infinity)
                    }
                    .buttonStyle(.glassProminent)
                    .tint(l.available < 0 ? tone.tint.color : Color.money)
                    .controlSize(.large)
                    .disabled(l.available == 0)
                }
                .listRowInsets(EdgeInsets())
                .listRowBackground(Color.clear)
            }

            Section {
                figure("Left Over from Last Month", l.carried)
                Button { present(.assign(l.categoryId)) } label: {
                    LabeledContent("Assigned This Month") { CentsText(l.assigned).foregroundStyle(Color.primary) }
                }
                .foregroundStyle(.primary)
                .accessibilityHint("Changes what this month assigns it")
                figure("Activity", l.activity)
                if l.creditOverspent > 0 {
                    LabeledContent("Spent on a Card, Not Covered") {
                        CentsText(l.creditOverspent).foregroundStyle(Color.orange)
                    }
                }
            } footer: {
                if l.creditOverspent > 0 {
                    Text("What the card bought beyond what \(l.name) had is new debt on the card, not cash gone: next month's money doesn't pay for it. Cover it here, or assign to the card's payment.")
                }
            }

            Section("Target") {
                if l.target > 0 {
                    HStack(spacing: Space.l) {
                        Gauge(value: funded(l) ?? 0) {
                            EmptyView()
                        } currentValueLabel: {
                            Text("\(Int(((funded(l) ?? 0) * 100).rounded()))%").font(.caption.weight(.semibold))
                        }
                        .gaugeStyle(.accessoryCircularCapacity)
                        .tint(tone == .short ? Color.orange : Color.green)
                        VStack(alignment: .leading, spacing: 2) {
                            Text("\(CentsText.string(l.target)) a month").font(.headline)
                            Text(l.assigned >= l.target ? "Funded this month" : "\(CentsText.string(Moves.toTarget(l))) more to meet it")
                                .font(.subheadline)
                                .foregroundStyle(l.assigned >= l.target ? Color.green : Color.secondary)
                        }
                    }
                    .padding(.vertical, Space.xs)
                    .accessibilityElement(children: .combine)
                    if l.assigned < l.target {
                        Button {
                            model.assign(l.categoryId, l.target)
                            assigned += 1
                        } label: {
                            Label("Assign \(CentsText.string(Moves.toTarget(l)))", systemImage: "target")
                        }
                    }
                    Button { present(.target(l.categoryId)) } label: { Label("Edit Target", systemImage: "scope") }
                } else {
                    Button { present(.target(l.categoryId)) } label: {
                        VStack(alignment: .leading, spacing: 2) {
                            Label("Set a Target", systemImage: "scope")
                            Text("How much does it need each month?").font(.footnote).foregroundStyle(.secondary)
                        }
                    }
                }
            }

            Section {
                quick("Assigned Last Month", l.assignedLastMonth, l)
                quick("Spent Last Month", -l.spentLastMonth, l)
                quick("Average Spent", -l.averageSpent, l)
            } header: {
                Text("Auto-Assign")
            } footer: {
                Text("Tap one to assign that much this month.")
            }

            if !spentHere.isEmpty {
                Section("This Month") {
                    ForEach(spentHere) { t in TxnRow(txn: t, showAccount: true) }
                }
            }

            Section {
                Button { typed = l.name; renaming = true } label: { Label("Rename", systemImage: "pencil") }
                Button { model.hide(category: l.categoryId, !l.hidden) } label: {
                    Label(l.hidden ? "Show in Budget" : "Hide from Budget", systemImage: l.hidden ? "eye" : "eye.slash")
                }
                Button(role: .destructive) { deleting = true } label: { Label("Delete Category", systemImage: "trash") }
            }
        }
        .listStyle(.insetGrouped)
        .navigationTitle(l.name)
        .navigationSubtitle(l.group)
        .toolbar { ToolbarItem(placement: .topBarTrailing) { AddTransactionButton() } }
        .onAppear { model.hold(register: nil) }
        .onDisappear { model.release(register: nil) }
        .sensoryFeedback(.success, trigger: assigned)
        .alert("Rename Category", isPresented: $renaming) {
            TextField("Name", text: $typed)
            Button("Cancel", role: .cancel) {}
            Button("Rename") { model.rename(category: l.categoryId, typed.trimmingCharacters(in: .whitespaces)) }
        }
        .confirmationDialog("Delete \(l.name)?", isPresented: $deleting, titleVisibility: .visible) {
            Button("Delete Category", role: .destructive) {
                model.delete(category: l.categoryId, moveTo: nil)
                dismiss()
            }
        } message: {
            Text("Its transactions will need a category again.")
        }
    }

    /// A card's payment category: the money set aside for the card large,
    /// with the one tap that covers what the card owes when it falls short;
    /// what the card owes and what of it is not covered; how the money got
    /// here; what went on the card this month; and the card's register. Its
    /// name is the card's and it goes when the card does: nothing to rename
    /// or delete, and no target — the debt is its target.
    private func payment(_ l: BudgetLine) -> some View {
        let tone = Tone.of(l)
        let onCard = (model.register(nil) ?? []).filter { $0.accountId == l.card && Months.of($0.date) == model.month }
        return List {
            Section {
                HeroFigure("Available for Payment", cents: l.available, tint: tone.tint, note: model.monthName) {
                    if let cover = Cards.toCover(l) {
                        Button {
                            model.assign(l.categoryId, cover)
                            assigned += 1
                        } label: {
                            Label("Assign \(CentsText.string(l.underfunded))", systemImage: "creditcard.fill")
                                .frame(maxWidth: .infinity)
                        }
                        .buttonStyle(.glassProminent)
                        .tint(Color.orange)
                        .controlSize(.large)
                        .accessibilityHint("Assigns what covers the card's debt")
                    } else {
                        Button {
                            present(.move(l.categoryId))
                        } label: {
                            Label("Move Money", systemImage: "arrow.left.arrow.right").frame(maxWidth: .infinity)
                        }
                        .buttonStyle(.glassProminent)
                        .tint(Color.money)
                        .controlSize(.large)
                        .disabled(l.available <= 0)
                    }
                }
                .listRowInsets(EdgeInsets())
                .listRowBackground(Color.clear)
            }

            Section {
                LabeledContent("Owed at the Month's End") { CentsText(l.debt) }
                LabeledContent("Not Covered") {
                    CentsText(l.underfunded).foregroundStyle(l.underfunded > 0 ? Color.orange : Color.green)
                }
                if let c = Cards.covered(l) {
                    ProgressView(value: c) {
                        EmptyView()
                    } currentValueLabel: {
                        Text(c >= 1 ? "All of it set aside" : "\(Int((c * 100).rounded()))% set aside")
                    }
                    .tint(c >= 1 ? Color.green : Color.orange)
                }
            } header: {
                Text("The Card")
            } footer: {
                Text(l.underfunded > 0 ? "What the card bought beyond what its categories had is debt. Assign money here to cover it."
                     : l.debt > 0 ? "Enough is set aside to pay the card." : "Nothing is owed on the card.")
            }

            Section {
                figure("Left Over from Last Month", l.carried)
                Button { present(.assign(l.categoryId)) } label: {
                    LabeledContent("Assigned This Month") { CentsText(l.assigned).foregroundStyle(Color.primary) }
                }
                .foregroundStyle(.primary)
                .accessibilityHint("Changes what this month assigns it")
                figure("Spending Covered, Less Payments", l.activity)
            } footer: {
                Text("Spending on the card moves what each category had for it here; paying the card spends from here.")
            }

            if !onCard.isEmpty {
                Section("On the Card This Month") {
                    ForEach(onCard) { t in TxnButton(txn: t) }
                }
            }

            Section {
                if let card = l.card {
                    NavigationLink(value: Route.register(card)) { Label("Card's Transactions", systemImage: "creditcard") }
                }
                Button { model.hide(category: l.categoryId, !l.hidden) } label: {
                    Label(l.hidden ? "Show in Budget" : "Hide from Budget", systemImage: l.hidden ? "eye" : "eye.slash")
                }
            } footer: {
                Text("Named as the card is, it goes when the card is closed.")
            }
        }
        .listStyle(.insetGrouped)
        .navigationTitle(l.name)
        .navigationSubtitle(l.group)
        .toolbar { ToolbarItem(placement: .topBarTrailing) { AddTransactionButton(account: l.card) } }
        .onAppear { model.hold(register: nil) }
        .onDisappear { model.release(register: nil) }
        .sensoryFeedback(.success, trigger: assigned)
    }

    private func figure(_ label: String, _ cents: Int64) -> some View {
        LabeledContent(label) { CentsText(cents).foregroundStyle(cents < 0 ? Color.red : Color.secondary) }
    }

    /// An Auto-Assign amount: tapped, the month assigns exactly that.
    private func quick(_ label: String, _ cents: Int64, _ l: BudgetLine) -> some View {
        Button {
            model.assign(l.categoryId, max(cents, 0))
            assigned += 1
        } label: {
            LabeledContent(label) { CentsText(max(cents, 0)) }
        }
        .foregroundStyle(.primary)
        .accessibilityHint("Assigns this much this month")
    }
}
