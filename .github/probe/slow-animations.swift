// The iOS simulator's own Slow Animations (Simulator.app, Debug ▸ Slow
// Animations), from the command line: CoreSimulator's
// -[SimDevice setCADebugOption:enabled:] with "debug_slow_animations", the
// call the menu makes, which slows the render server's clock, so every
// animation on the device slows, the system's own transitions included.
// CoreSimulator is private, so it is reached through the Objective-C runtime.
//
//   xcrun swift slow-animations.swift <udid> on|off
import Foundation

let args = CommandLine.arguments
guard args.count == 3, ["on", "off"].contains(args[2]) else {
    FileHandle.standardError.write("usage: slow-animations.swift <udid> on|off\n".data(using: .utf8)!)
    exit(2)
}
let (udid, on) = (args[1], args[2] == "on")

func fail(_ why: String) -> Never {
    FileHandle.standardError.write("slow-animations: \(why)\n".data(using: .utf8)!)
    exit(1)
}

func developerDir() -> String {
    let xcodeSelect = Process()
    xcodeSelect.executableURL = URL(fileURLWithPath: "/usr/bin/xcode-select")
    xcodeSelect.arguments = ["-p"]
    let out = Pipe()
    xcodeSelect.standardOutput = out
    try? xcodeSelect.run()
    xcodeSelect.waitUntilExit()
    let path = String(data: out.fileHandleForReading.readDataToEndOfFile(), encoding: .utf8) ?? ""
    return path.trimmingCharacters(in: .whitespacesAndNewlines)
}

let dev = developerDir()
for path in [
    "/Library/Developer/PrivateFrameworks/CoreSimulator.framework/CoreSimulator",
    "\(dev)/Library/PrivateFrameworks/CoreSimulator.framework/CoreSimulator",
] {
    _ = dlopen(path, RTLD_NOW)
}

guard let contextClass = NSClassFromString("SimServiceContext") as? NSObject.Type else { fail("no CoreSimulator") }
guard let context = contextClass.perform(NSSelectorFromString("sharedServiceContextForDeveloperDir:error:"), with: dev, with: nil)?
    .takeUnretainedValue() as? NSObject,
    let deviceSet = context.perform(NSSelectorFromString("defaultDeviceSetWithError:"), with: nil)?
    .takeUnretainedValue() as? NSObject,
    let devices = deviceSet.value(forKey: "devices") as? [NSObject]
else { fail("no device set") }
guard let device = devices.first(where: { ($0.value(forKey: "UDID") as? NSUUID)?.uuidString == udid }) else {
    fail("no device \(udid)")
}

let set = NSSelectorFromString("setCADebugOption:enabled:")
guard device.responds(to: set) else { fail("this CoreSimulator has no setCADebugOption:enabled:") }
typealias SetOption = @convention(c) (AnyObject, Selector, NSString, ObjCBool) -> ObjCBool
let setOption = unsafeBitCast(device.method(for: set), to: SetOption.self)
guard setOption(device, set, "debug_slow_animations", ObjCBool(on)).boolValue else {
    fail("the simulator refused debug_slow_animations \(args[2])")
}
print("slow animations \(args[2]) on \(udid)")
