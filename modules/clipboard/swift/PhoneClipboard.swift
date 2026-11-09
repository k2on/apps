// ark-clipboard on the iPhone and the Mac (modules/clipboard/README.md):
// the `Clipboard` protocol UniFFI writes, over the general pasteboard.
import Foundation
#if canImport(UIKit)
import UIKit
#elseif canImport(AppKit)
import AppKit
#endif

/// The general pasteboard.
final class PhoneClipboard: Clipboard, @unchecked Sendable {
    func copyText(text: String) {
        DispatchQueue.main.async {
            #if canImport(UIKit)
            UIPasteboard.general.string = text
            #elseif canImport(AppKit)
            NSPasteboard.general.clearContents()
            NSPasteboard.general.setString(text, forType: .string)
            #endif
        }
    }
}
