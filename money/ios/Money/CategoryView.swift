// A category's month: the desktop's inspector, as a page of its own. Its
// available balance large at the top in its tone, how it got there (left
// over from last month, assigned, spent), the one thing to do about it —
// Cover Overspending when it is below nothing, Move Money otherwise — its
// target with a ring of how far the month is toward it and the button that
// meets it, Auto-Assign's three amounts as one-tap choices, the month's
// transactions in it, and managing it (rename, hide, delete) at the end,
// as Settings' rows put the destructive one last.
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
            page(l)
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
                    .tint(l.available < 0 ? Color.red : Color.money)
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
