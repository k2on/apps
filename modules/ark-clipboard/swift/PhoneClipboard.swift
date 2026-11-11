// ark-clipboard on the iPhone and the Mac (modules/ark-clipboard/README.md):
// the `Clipboard` protocol UniFFI writes, over the general pasteboard.
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

/// The general pasteboard.
public final class PhoneClipboard: Clipboard, @unchecked Sendable {
    public init() {}

    public func copyText(text: String) {
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
