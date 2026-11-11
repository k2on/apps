// ark-haptics on the iPhone and the Mac (modules/ark-haptics/README.md): the
// foreign trait UniFFI writes as the `Haptics` protocol, implemented over
// the system's feedback generators. An app that lists `ark-haptics` in its
// app.toml links it (under nix a module of its own, `ArkHaptics`, over the
// app's bindings; in Xcode compiled into the app) and registers one at
// launch.
import Foundation
// The app's bindings, where nix compiles this as a module of its own over
// them (flake.nix, `iosApp`); an app's own Xcode project compiles it beside
// them, and there is no such module. Public for the app it is linked into.
#if canImport(ArkBindings)
import ArkBindings
#endif
#if canImport(UIKit)
import UIKit
#elseif canImport(AppKit)
import AppKit
#endif

/// `ui/docs/motion.md`'s haptics: a success, a selection step, a warning.
public final class PhoneHaptics: Haptics, @unchecked Sendable {
    public init() {}

    public func perform(haptic: Haptic) {
        DispatchQueue.main.async {
            #if canImport(UIKit)
            switch haptic {
            case .success: UINotificationFeedbackGenerator().notificationOccurred(.success)
            case .warning: UINotificationFeedbackGenerator().notificationOccurred(.warning)
            case .selection: UISelectionFeedbackGenerator().selectionChanged()
            }
            #elseif canImport(AppKit)
            // A Force Touch trackpad's answer: the Mac has one weight for a
            // step and one for a change kept.
            let p = NSHapticFeedbackManager.defaultPerformer
            switch haptic {
            case .selection: p.perform(.alignment, performanceTime: .now)
            case .success, .warning: p.perform(.levelChange, performanceTime: .now)
            }
            #endif
        }
    }
}
