// The iPhone's implementations of the device modules a screen run in Rust
// asks for (`ark_modules`, ui/docs/app.md): each one the foreign trait
// UniFFI writes as a Swift protocol, registered once at launch
// (MoneyApp.swift). The camera's QR scan stays the hand-written page it was
// (`QrScanner`, the join sheet) until a screen that scans moves to Rust.
import UIKit

/// `ui/docs/motion.md`'s haptics: a success, a selection step, a warning.
final class PhoneHaptics: Haptics, @unchecked Sendable {
    func perform(haptic: Haptic) {
        DispatchQueue.main.async {
            switch haptic {
            case .success: UINotificationFeedbackGenerator().notificationOccurred(.success)
            case .warning: UINotificationFeedbackGenerator().notificationOccurred(.warning)
            case .selection: UISelectionFeedbackGenerator().selectionChanged()
            }
        }
    }
}

/// The general pasteboard.
final class PhoneClipboard: Clipboard, @unchecked Sendable {
    func copyText(text: String) {
        DispatchQueue.main.async { UIPasteboard.general.string = text }
    }
}

/// The system's share sheet, over whatever is in front.
final class PhoneShare: Share, @unchecked Sendable {
    func share(text: String, url: String?) {
        DispatchQueue.main.async {
            var items: [Any] = [text]
            if let u = url.flatMap(URL.init(string:)) { items.append(u) }
            let sheet = UIActivityViewController(activityItems: items, applicationActivities: nil)
            let scenes = UIApplication.shared.connectedScenes.compactMap { $0 as? UIWindowScene }
            var top = scenes.first?.keyWindow?.rootViewController
            while let next = top?.presentedViewController { top = next }
            top?.present(sheet, animated: true)
        }
    }
}
