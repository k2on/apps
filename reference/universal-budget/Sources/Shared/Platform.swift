import SwiftUI

// The few places the app needs to know which platform it is on. Everything
// else adapts through SwiftUI's own containers.

/// Whether the view is in a compact-width environment: an iPhone in
/// portrait, or a narrow iPad split-view or Slide Over window. Macs are never
/// compact; their windows have a minimum size instead.
@propertyWrapper
struct IsCompact: DynamicProperty {
    #if os(iOS)
    @Environment(\.horizontalSizeClass) private var sizeClass
    var wrappedValue: Bool { sizeClass == .compact }
    #else
    var wrappedValue: Bool { false }
    #endif
}

extension View {
    /// A number pad for amounts on iPhone and iPad; nothing to do on Mac.
    func decimalKeyboard() -> some View {
        #if os(iOS)
        keyboardType(.decimalPad)
        #else
        self
        #endif
    }

    /// Mac sheets size to their content, and a grouped Form has no intrinsic
    /// size, so give it one. iPhone and iPad sheets size themselves.
    func sheetFrame(width: CGFloat = 480, height: CGFloat = 600) -> some View {
        #if os(macOS)
        frame(minWidth: width, idealWidth: width, minHeight: height, idealHeight: height)
        #else
        self
        #endif
    }
}

/// Builds a SwiftUI image from receipt photo data.
extension Image {
    init?(receiptData data: Data) {
        #if os(macOS)
        guard let image = NSImage(data: data) else { return nil }
        self.init(nsImage: image)
        #else
        guard let image = UIImage(data: data) else { return nil }
        self.init(uiImage: image)
        #endif
    }
}
