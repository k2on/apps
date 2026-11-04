// The app XCUITest needs a UI test to belong to. The test drives another.
import SwiftUI

@main
struct Host: App {
    var body: some Scene { WindowGroup { Text("") } }
}
