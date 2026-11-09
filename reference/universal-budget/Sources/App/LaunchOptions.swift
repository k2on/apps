import Foundation

/// Launch arguments that open the app on a particular screen, so CI can
/// take screenshots without UI automation. See SCREENS.md.
///
/// Arguments are read through `UserDefaults`, which puts `-key value` pairs
/// from the command line in its argument domain. That is also how
/// `-appearance dark` reaches the `@AppStorage("appearance")` setting
/// without any code here.
struct LaunchOptions {
    enum Screen: Equatable {
        case budget
        case category(String)
        case accounts
        case account(String)
        case reconcile(String)
        case addAccount
        case transactions
        case review
        case transaction(String)
        case editTransaction(String)
        case addTransaction
        case insights
        case settings

        init?(argument: String) {
            let parts = argument.split(separator: ":", maxSplits: 1).map(String.init)
            let value = parts.count > 1 ? parts[1] : ""
            switch (parts.first?.lowercased() ?? "", value.isEmpty) {
            case ("budget", _): self = .budget
            case ("category", false): self = .category(value)
            case ("accounts", _): self = .accounts
            case ("account", false): self = .account(value)
            case ("reconcile", false): self = .reconcile(value)
            case ("add-account", _): self = .addAccount
            case ("transactions", _): self = .transactions
            case ("review", _): self = .review
            case ("transaction", false): self = .transaction(value)
            case ("edit-transaction", false): self = .editTransaction(value)
            case ("add-transaction", _): self = .addTransaction
            case ("insights", _): self = .insights
            case ("settings", _): self = .settings
            default: return nil
            }
        }
    }

    var screen: Screen?
    /// `-inspector 1`: open the inspector on screens that have one.
    var showsInspector: Bool
    /// `-columns all`: show the sidebar on iPad instead of the tab bar.
    var showsAllColumns: Bool
    /// `-month 2026-10`: the budget month to open.
    var month: Month?
    /// `-search "greenleaf"`: initial transaction search text.
    var search: String?
    /// `-window 1440x900`: initial Mac window size.
    var windowSize: CGSize?

    static let current = LaunchOptions(defaults: .standard)

    init(defaults: UserDefaults) {
        screen = defaults.string(forKey: "screen").flatMap(Screen.init(argument:))
        showsInspector = defaults.bool(forKey: "inspector")
        showsAllColumns = defaults.string(forKey: "columns") == "all"
        month = defaults.string(forKey: "month").flatMap(Month.init(argument:))
        search = defaults.string(forKey: "search")
        windowSize = defaults.string(forKey: "window").flatMap { value in
            let parts = value.lowercased().split(separator: "x").compactMap { Double($0) }
            return parts.count == 2 ? CGSize(width: parts[0], height: parts[1]) : nil
        }
    }
}
