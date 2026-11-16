// The runner's display made big enough for a wide window (app-mac.sh):
// the main display's modes said, then the smallest that holds
// `<width>x<height>` points, at twice the pixels where there is one
// (HiDPI, what a person's Mac draws in), else at its own, set for good.
// The runner's display was 1024x768 at one pixel a point: the Mac's
// `-window 1440x900` came out a 1024x677 window, at half the pixels of the
// desktop's and the browser's (app-screens 43796348626). Says the mode it
// is in after.
//
//   display <width> <height>
import CoreGraphics
import Foundation

let args = CommandLine.arguments
let want = (w: Int(args.count > 1 ? args[1] : "1440") ?? 1440, h: Int(args.count > 2 ? args[2] : "900") ?? 900)
let display = CGMainDisplayID()
let options = [kCGDisplayShowDuplicateLowResolutionModes: kCFBooleanTrue] as CFDictionary
let modes = (CGDisplayCopyAllDisplayModes(display, options) as? [CGDisplayMode]) ?? []
func said(_ m: CGDisplayMode) -> String { "\(m.width)x\(m.height) at \(m.pixelWidth)x\(m.pixelHeight)" }
if let now = CGDisplayCopyDisplayMode(display) { print("now: \(said(now))") }
print("modes: \(modes.map(said).joined(separator: ", "))")
let fits = modes.filter { $0.width >= want.w && $0.height >= want.h && $0.isUsableForDesktopGUI() }
let hidpi = fits.filter { $0.pixelWidth >= 2 * $0.width }
let pick = (hidpi.isEmpty ? fits : hidpi).min { $0.width * $0.height < $1.width * $1.height }
guard let pick else {
    print("no mode holds \(want.w)x\(want.h); left as it is")
    exit(0)
}
var config: CGDisplayConfigRef?
guard CGBeginDisplayConfiguration(&config) == .success else { print("no configuration"); exit(0) }
CGConfigureDisplayWithDisplayMode(config, display, pick, nil)
let done = CGCompleteDisplayConfiguration(config, .permanently)
if let now = CGDisplayCopyDisplayMode(display) { print("set \(said(pick)): \(done == .success ? "done" : "refused (\(done.rawValue))"); now \(said(now))") }
