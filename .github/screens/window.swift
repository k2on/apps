// The id of a process's window, for `screencapture -l` (app-mac.sh): its
// frontmost window on the screen at the normal level (the window server
// lists them front to back), or none (exit 1).
//
//   window <pid>
import CoreGraphics
import Foundation

guard CommandLine.arguments.count > 1, let pid = Int(CommandLine.arguments[1]) else { exit(2) }
let windows = CGWindowListCopyWindowInfo([.optionOnScreenOnly, .excludeDesktopElements], kCGNullWindowID) as? [[String: Any]] ?? []
for w in windows {
    guard (w[kCGWindowOwnerPID as String] as? Int) == pid, (w[kCGWindowLayer as String] as? Int) == 0,
          let id = w[kCGWindowNumber as String] as? Int else { continue }
    print(id)
    exit(0)
}
exit(1)
