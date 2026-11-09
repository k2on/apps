// What one platform has and the others do not, asked here and nowhere in
// an app (ui/docs/design.md: `#if os` only inside ArkUI): a title inline in
// its bar, a grouped list, the keypad a field brings up, how a field
// capitalises, a cover over everything, a picture's JPEG.
#if canImport(SwiftUI)
import SwiftUI
#if canImport(UIKit)
import UIKit
#elseif canImport(AppKit)
import AppKit
#endif

/// The keypad a field asks for.
public enum Keypad {
    case decimal, number, url, numbersAndPunctuation
}

/// How a field capitalises what is typed.
public enum Capitals {
    case never, words, characters
}

extension View {
    /// The title small, in the bar (the iPhone's and the iPad's); the Mac's always is.
    public func inlineTitle() -> some View {
        #if os(iOS)
        navigationBarTitleDisplayMode(.inline)
        #else
        self
        #endif
    }

    /// A grouped list: inset and grouped on the iPhone and the iPad, inset on the Mac.
    public func groupedList() -> some View {
        #if os(iOS)
        listStyle(.insetGrouped)
        #else
        listStyle(.inset)
        #endif
    }

    /// The keypad a field brings up on a phone; the Mac has its keyboard.
    public func keypad(_ k: Keypad) -> some View {
        #if os(iOS)
        keyboardType({
            switch k {
            case .decimal: return UIKeyboardType.decimalPad
            case .number: return .numberPad
            case .url: return .URL
            case .numbersAndPunctuation: return .numbersAndPunctuation
            }
        }())
        #else
        self
        #endif
    }

    /// How a field capitalises on a phone's keyboard.
    public func capitals(_ c: Capitals) -> some View {
        #if os(iOS)
        textInputAutocapitalization({
            switch c {
            case .never: return TextInputAutocapitalization.never
            case .words: return .words
            case .characters: return .characters
            }
        }())
        #else
        self
        #endif
    }

    /// Over everything: a full-screen cover on the iPhone and the iPad, a sheet on the Mac.
    public func cover<Item: Identifiable, Content: View>(item: Binding<Item?>, @ViewBuilder content: @escaping (Item) -> Content) -> some View {
        #if os(iOS)
        fullScreenCover(item: item, content: content)
        #else
        sheet(item: item, content: content)
        #endif
    }
}

/// A picture's bytes drawn: an image, or none when they are not one.
public func picture(_ data: Data) -> Image? {
    #if canImport(UIKit)
    return UIImage(data: data).map { Image(uiImage: $0) }
    #elseif canImport(AppKit)
    return NSImage(data: data).map { Image(nsImage: $0) }
    #else
    return nil
    #endif
}

/// A picture's bytes as a JPEG, whatever they were (HEIC from the camera).
public func jpeg(_ data: Data, quality: Double = 0.8) -> Data? {
    #if canImport(UIKit)
    return UIImage(data: data)?.jpegData(compressionQuality: quality)
    #elseif canImport(AppKit)
    guard let rep = NSBitmapImageRep(data: data) else { return nil }
    return rep.representation(using: .jpeg, properties: [.compressionFactor: quality])
    #else
    return nil
    #endif
}
#endif
