// The runner's main display set to the largest mode it offers (the image
// starts at 1024x768, too small for a 1440x900 window), at 1x where there is
// a choice. Prints the mode it ends in.
//
//   display
import CoreGraphics
import Foundation

let display = CGMainDisplayID()
let options = [kCGDisplayShowDuplicateLowResolutionModes: kCFBooleanTrue] as CFDictionary
let modes = (CGDisplayCopyAllDisplayModes(display, options) as? [CGDisplayMode]) ?? []
for m in modes { print("mode \(m.width)x\(m.height) pixels \(m.pixelWidth)x\(m.pixelHeight) usable \(m.isUsableForDesktopGUI())") }
let best = modes
    .filter { $0.isUsableForDesktopGUI() }
    .max { ($0.width * $0.height, -$0.pixelWidth) < ($1.width * $1.height, -$1.pixelWidth) }
if let best, best.width > CGDisplayPixelsWide(display) {
    var config: CGDisplayConfigRef?
    CGBeginDisplayConfiguration(&config)
    CGConfigureDisplayWithDisplayMode(config, display, best, nil)
    let error = CGCompleteDisplayConfiguration(config, .permanently)
    print("set \(best.width)x\(best.height): \(error.rawValue)")
}
print("display \(CGDisplayPixelsWide(display))x\(CGDisplayPixelsHigh(display))")
