import SwiftUI

/// A menu of every category, grouped, for categorising from a context menu
/// or the menu bar. Takes the groups explicitly because menu-bar commands
/// don't receive a window's environment.
struct CategoryMenu: View {
    var title: LocalizedStringKey
    var groups: [CategoryGroup]
    var includesReadyToAssign = true
    var action: (Category.ID) -> Void

    init(
        _ title: LocalizedStringKey, groups: [CategoryGroup], includesReadyToAssign: Bool = true,
        action: @escaping (Category.ID) -> Void
    ) {
        self.title = title
        self.groups = groups
        self.includesReadyToAssign = includesReadyToAssign
        self.action = action
    }

    var body: some View {
        Menu(title) {
            if includesReadyToAssign {
                Button("Ready to Assign") { action(Category.readyToAssign) }
                Divider()
            }
            ForEach(groups) { group in
                Section(group.name) {
                    ForEach(group.categories) { category in
                        Button(category.name) { action(category.id) }
                    }
                }
            }
        }
    }
}

/// A picker of categories, grouped into sections, for forms.
struct CategoryPicker: View {
    var title: LocalizedStringKey = "Category"
    @Binding var selection: Category.ID?
    var includesReadyToAssign = true

    @Environment(BudgetStore.self) private var store

    var body: some View {
        Picker(title, selection: $selection) {
            Text("Uncategorized").tag(Category.ID?.none)
            if includesReadyToAssign {
                Text("Ready to Assign").tag(Category.ID?.some(Category.readyToAssign))
            }
            ForEach(store.groups) { group in
                Section(group.name) {
                    ForEach(group.categories) { category in
                        Text(category.name).tag(Category.ID?.some(category.id))
                    }
                }
            }
        }
    }
}
