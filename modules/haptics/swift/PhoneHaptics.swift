// ark-haptics on the iPhone and the Mac (modules/haptics/README.md): the
// foreign trait UniFFI writes as the `Haptics` protocol, implemented over
// the system's feedback generators. An app that lists `haptics` in its
// modules.toml compiles this file in and registers one at launch.
import Foundation
#if canImport(UIKit)
import UIKit
#elseif canImport(AppKit)
import AppKit
#endif

/// `ui/docs/motion.md`'s haptics: a success, a selection step, a warning.
final class PhoneHaptics: Haptics, @unchecked Sendable {
    func perform(haptic: Haptic) {
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
