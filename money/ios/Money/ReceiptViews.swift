// A transaction's receipt, in its form: the receipt drawn as the paper is —
// monospace on white, whatever the phone's appearance — with a word when
// its reading did not add up, its photo a tap away, and what can be done
// with it (put it right, take it off). With none: scan one as Notes scans a
// document (VisionKit's camera, which finds the page's edges, takes it when
// it is steady and squares it up), choose a photo of one, type one, or
// match one nobody has. A photo goes to the phone's files and the server
// reads it (Bridge.addPhoto); until it has, the section says it is reading.
// The rules are Receipts.swift's.
import ArkUI
import PhotosUI
import SwiftUI

extension ReceiptDraft: Identifiable {
    var id: String { receiptId.isEmpty ? "new" : receiptId }
}

/// What a transaction's form has asked of its receipt: the scanner up, a
/// photo chosen, or the receipt form over a receipt typed or put right.
/// The form holds it, so that each is presented once, from the form and
/// not from a row of it.
struct ReceiptActs {
    var scanning = false
    var picked: PhotosPickerItem? = nil
    var typing: ReceiptDraft? = nil
    var kept = 0
}

/// On a transaction's form: what paid for it and each receipt's items held
/// while the form is up, and what its receipt section asks for presented —
/// the scanner, the library's photo sent, the receipt form.
struct ReceiptSheets: ViewModifier {
    let txn: Id?
    @Binding var acts: ReceiptActs
    @EnvironmentObject private var model: Model

    func body(content: Content) -> some View {
        content
            .holding(txn.map { MoneyView.paid(txn: $0) })
            .sheet(isPresented: $acts.scanning) {
                DocumentScanner { jpeg in
                    acts.scanning = false
                    if let jpeg { keep(jpeg) }
                }
                .ignoresSafeArea()
            }
            .onChange(of: acts.picked) { _, item in
                guard let item else { return }
                acts.picked = nil
                Task {
                    // A photo in whatever the library keeps it as (HEIC), sent as JPEG.
                    if let d = try? await item.loadTransferable(type: Data.self),
                       let j = jpeg(d) {
                        keep(j)
                    }
                }
            }
            .sheet(item: $acts.typing) { d in
                ReceiptForm(draft: d)
                    .presentationDetents([.large])
            }
            .sensoryFeedback(.success, trigger: acts.kept)
    }

    private func keep(_ jpeg: Data) {
        model.addPhoto(jpeg, txn: txn) { ok in if ok { acts.kept += 1 } }
    }
}

/// The receipt section of a transaction's form.
struct ReceiptSection: View {
    let txn: Id
    /// The transaction's amount, our sign, and its day and payee: what a
    /// receipt typed starts from and what one matched is nearest to.
    let amount: Int64
    let day: String
    let payee: String
    @Binding var acts: ReceiptActs
    @EnvironmentObject private var model: Model

    var body: some View {
        let s = model.snapshot
        let state = Receipts.state(txn: txn, receipts: model.paid(txn), photos: s.photos)
        Section {
            switch state {
            case .none:
                Text("No receipt yet").foregroundStyle(.quiet)
            case let .reading(why):
                HStack(spacing: Space.m) {
                    ProgressView()
                    VStack(alignment: .leading, spacing: 2) {
                        Text("Reading the receipt…")
                        Text(why.isEmpty ? "Your server reads it once the photo reaches it." : "Not read yet: \(why)")
                            .font(.caption).foregroundStyle(.quiet)
                    }
                }
                if let p = s.photos.first(where: { $0.txnId == txn && $0.receiptId.isEmpty }) {
                    NavigationLink { ReceiptPhotoView(file: p.file) } label: { Label("Photo", systemImage: "photo") }
                }
            case let .receipt(r, check):
                if check {
                    Label("Check this: it doesn't add up as read", systemImage: "exclamationmark.triangle.fill")
                        .foregroundStyle(Color.caution)
                }
                PaperReceipt(receipt: r, items: model.items(r.id))
                    .holding(MoneyView.items(receipt: r.id))
                    .listRowInsets(EdgeInsets(top: Space.m, leading: 0, bottom: Space.m, trailing: 0))
                    .listRowBackground(Color.clear)
                if let p = Receipts.photo(of: r.id, s.photos) {
                    NavigationLink { ReceiptPhotoView(file: p.file) } label: { Label("Photo", systemImage: "photo") }
                }
                Button { acts.typing = ReceiptDraft(r, items: model.items(r.id), txn: txn) } label: { Label("Edit Receipt", systemImage: "pencil") }
                Button(role: .destructive) { model.pay(receipt: r.id, with: nil) } label: { Label("Take Receipt Off", systemImage: "minus.circle") }
            }
            if !state.hasReceipt { adding(s) }
        } header: {
            Text("Receipt")
        }
    }

    /// The ways to give a transaction its receipt.
    @ViewBuilder private func adding(_ s: Snapshot) -> some View {
        if DocumentScanner.isSupported {
            Button { acts.scanning = true } label: { Label("Scan Receipt", systemImage: "doc.viewfinder") }
        }
        PhotosPicker(selection: $acts.picked, matching: .images) { Label("Choose Photo", systemImage: "photo.on.rectangle") }
        Button { acts.typing = ReceiptDraft(txn: txn, place: payee, date: day) } label: { Label("Type Receipt", systemImage: "keyboard") }
        let free = Receipts.unmatched(s.receipts, amount: amount, day: day)
        if !free.isEmpty {
            NavigationLink { MatchReceiptList(txn: txn, receipts: free) } label: {
                Label("Match a Receipt (\(free.count))", systemImage: "link")
            }
        }
    }
}


/// A receipt as the paper is: the store and day centred, each item with
/// what it cost against the right, a rule, and the total — black monospace
/// on white in light and dark alike.
struct PaperReceipt: View {
    let receipt: ReceiptLine
    let items: [ItemLine]

    var body: some View {
        VStack(alignment: .leading, spacing: 3) {
            Text(Receipts.heading(receipt)).fontWeight(.bold).frame(maxWidth: .infinity)
            Text(receipt.date).frame(maxWidth: .infinity)
            rule
            ForEach(items, id: \.line) { i in
                HStack(alignment: .firstTextBaseline) {
                    Text(i.name).lineLimit(2)
                    Spacer(minLength: Space.m)
                    Text(ReceiptDraft.typed(-i.amount))
                }
                if i.quantity != 1000 {
                    Text("  \(formatQuantity(thousandths: i.quantity)) \(i.unit)").foregroundStyle(Color.black.opacity(0.55))
                }
            }
            if items.isEmpty && receipt.items > 0 { Text("…").frame(maxWidth: .infinity) }
            rule
            HStack {
                Text("TOTAL").fontWeight(.bold)
                Spacer()
                Text(ReceiptDraft.typed(-receipt.total)).fontWeight(.bold)
            }
        }
        .font(.system(.footnote, design: .monospaced))
        .foregroundStyle(Color.black)
        .padding(Space.l)
        .background(Color.white, in: RoundedRectangle(cornerRadius: 4))
        .shadow(color: .black.opacity(0.18), radius: 6, y: 2)
        .padding(.horizontal, Space.l)
        .environment(\.colorScheme, .light)
        .accessibilityElement(children: .combine)
    }

    private var rule: some View {
        Text(String(repeating: "-", count: 40)).lineLimit(1).truncationMode(.tail).foregroundStyle(Color.black.opacity(0.5))
    }

}

/// A receipt typed by hand, or one already read put right: where, when, and
/// each item with what it cost.
struct ReceiptForm: View {
    @State var draft: ReceiptDraft
    @EnvironmentObject private var model: Model
    @Environment(\.dismiss) private var dismiss

    var body: some View {
        NavigationStack {
            Form {
                Section {
                    TextField("Store", text: $draft.place).capitals(.words)
                    DatePicker("Date", selection: Binding(get: { Days.date(draft.date) ?? Date() }, set: { draft.date = Days.day($0) }),
                               displayedComponents: .date)
                }
                Section {
                    ForEach($draft.items) { $i in
                        HStack {
                            TextField("Item", text: $i.name).capitals(.characters)
                            TextField("0.00", text: $i.amount)
                                .keypad(.numbersAndPunctuation)
                                .multilineTextAlignment(.trailing)
                                .font(.body.monospacedDigit())
                                .frame(maxWidth: 96)
                        }
                    }
                    .onDelete { draft.items.remove(atOffsets: $0) }
                    Button { draft.items.append(PaperItem()) } label: { Label("Add Item", systemImage: "plus") }
                } header: {
                    Text("Items")
                } footer: {
                    Text("A discount is below zero, -0.50.")
                }
                Section {
                    LabeledContent("Total") { Text(ReceiptDraft.typed(draft.total)).font(.body.monospacedDigit()) }
                } footer: {
                    if let p = draft.problem { Text(p) }
                }
            }
            .navigationTitle(draft.receiptId.isEmpty ? "New Receipt" : "Receipt")
            .inlineTitle()
            .toolbar {
                ToolbarItem(placement: .cancellationAction) {
                    Button(role: .cancel) { dismiss() } label: { Image(systemName: "xmark") }
                        .accessibilityLabel("Cancel")
                }
                ToolbarItem(placement: .confirmationAction) {
                    Button(role: .confirm) {
                        model.save(draft) { ok in if ok { dismiss() } }
                    } label: { Image(systemName: "checkmark") }
                        .accessibilityLabel("Save")
                        .disabled(draft.problem != nil)
                }
            }
        }
    }
}

/// The receipts nobody has matched, the nearest to the transaction first:
/// one tapped is what paid for it.
struct MatchReceiptList: View {
    let txn: Id
    let receipts: [ReceiptLine]
    @EnvironmentObject private var model: Model
    @Environment(\.dismiss) private var dismiss

    var body: some View {
        List(receipts, id: \.id) { r in
            Button {
                model.pay(receipt: r.id, with: txn)
                dismiss()
            } label: {
                HStack {
                    VStack(alignment: .leading, spacing: 2) {
                        Text(Receipts.heading(r))
                        Text("\(r.date) · \(r.items) items").font(.caption).foregroundStyle(.quiet)
                    }
                    Spacer()
                    CentsText(r.total).font(.body.weight(.medium))
                }
            }
            .foregroundStyle(.ink)
        }
        .navigationTitle("Match a Receipt")
        .inlineTitle()
    }
}

/// A receipt's photo, whole: from the phone's files, or fetched from the
/// server the first time it is opened.
struct ReceiptPhotoView: View {
    let file: FileRef
    @EnvironmentObject private var model: Model

    var body: some View {
        Group {
            if let d = model.file(file), let image = picture(d) {
                ScrollView([.horizontal, .vertical]) {
                    image.resizable().scaledToFit().containerRelativeFrame(.horizontal)
                }
            } else {
                VStack(spacing: Space.m) {
                    ProgressView()
                    Text("Fetching the photo…").foregroundStyle(.quiet)
                }
            }
        }
        .navigationTitle("Receipt Photo")
        .inlineTitle()
    }
}
