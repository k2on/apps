import PhotosUI
import SwiftUI

/// A transaction's receipt: a photo, an itemised list, or both. Attaching a
/// photo works from the detail view; itemising is in the editor.
struct ReceiptSection: View {
    var transaction: Transaction

    @Environment(BudgetStore.self) private var store
    @Environment(\.currencyCode) private var currencyCode
    @State private var pickedItem: PhotosPickerItem?

    var body: some View {
        let receipt = transaction.receipt ?? Receipt()
        Section {
            if let data = receipt.imageData, let image = Image(receiptData: data) {
                image
                    .resizable()
                    .scaledToFit()
                    .frame(maxHeight: 280)
                    .clipShape(.rect(cornerRadius: 8))
                    .accessibilityLabel("Receipt photo")
            }

            ForEach(receipt.items) { item in
                ReceiptItemRow(item: item)
            }

            if !receipt.items.isEmpty {
                LabeledContent {
                    AmountText(receipt.itemsTotal).fontWeight(.semibold)
                } label: {
                    Text("Items Total")
                    if receipt.itemsTotal != abs(transaction.amount) {
                        Text("Differs from the transaction by \((abs(transaction.amount) - receipt.itemsTotal).formatted(currency: currencyCode))")
                            .foregroundStyle(.orange)
                    }
                }
            }

            PhotosPicker(selection: $pickedItem, matching: .images) {
                Label(receipt.imageData == nil ? "Attach Photo" : "Replace Photo", systemImage: "photo.badge.plus")
            }
            if receipt.imageData != nil {
                Button("Remove Photo", systemImage: "trash", role: .destructive) {
                    setImage(nil)
                }
            }
        } header: {
            Text("Receipt")
        } footer: {
            if receipt.isEmpty {
                Text("Attach a photo of the receipt, or itemise it in Edit.")
            }
        }
        .onChange(of: pickedItem) { _, item in
            guard let item else { return }
            Task {
                if let data = try? await item.loadTransferable(type: Data.self) {
                    setImage(data)
                }
                pickedItem = nil
            }
        }
    }

    private func setImage(_ data: Data?) {
        guard var updated = store.transaction(transaction.id) else { return }
        var receipt = updated.receipt ?? Receipt()
        receipt.imageData = data
        updated.receipt = receipt.isEmpty ? nil : receipt
        store.update(updated)
    }
}

struct ReceiptItemRow: View {
    var item: ReceiptItem

    @Environment(\.currencyCode) private var currencyCode

    var body: some View {
        LabeledContent {
            AmountText(item.total)
        } label: {
            Text(item.name)
            if item.quantity > 1 {
                Text("\(item.quantity) × \(item.price.formatted(currency: currencyCode))")
            }
        }
    }
}
