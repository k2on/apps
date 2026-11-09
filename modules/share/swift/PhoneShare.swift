// ark-share on the iPhone and the Mac (modules/share/README.md): the
// `Share` protocol UniFFI writes, over the system's share sheet.
import Foundation
#if canImport(UIKit)
import UIKit
#elseif canImport(AppKit)
import AppKit
#endif

/// The system's share sheet, over whatever is in front.
final class PhoneShare: Share, @unchecked Sendable {
    func share(text: String, url: String?) {
        DispatchQueue.main.async {
            var items: [Any] = [text]
            if let u = url.flatMap(URL.init(string:)) { items.append(u) }
            #if canImport(UIKit)
            let sheet = UIActivityViewController(activityItems: items, applicationActivities: nil)
            let scenes = UIApplication.shared.connectedScenes.compactMap { $0 as? UIWindowScene }
            var top = scenes.first?.keyWindow?.rootViewController
            while let next = top?.presentedViewController { top = next }
            // On an iPad the sheet is a popover, and needs somewhere to point.
            if let pop = sheet.popoverPresentationController, let view = top?.view {
                pop.sourceView = view
                pop.sourceRect = CGRect(x: view.bounds.midX, y: view.bounds.midY, width: 0, height: 0)
                pop.permittedArrowDirections = []
            }
            top?.present(sheet, animated: true)
            #elseif canImport(AppKit)
            guard let view = NSApp.keyWindow?.contentView else { return }
            NSSharingServicePicker(items: items).show(relativeTo: .zero, of: view, preferredEdge: .minY)
            #endif
        }
    }
}
