// The main display's modes, and a way to pick the sharpest tall one. With no argument it prints
// the current mode, then every mode, one a line: width, height (points), pixel width, pixel
// height. With `best` it switches to the HiDPI mode (two pixels a point) of the largest height
// it offers, and prints the mode it set or why it did not.
// The cloud shoot runs it (`swift scripts/shoot/display.swift best`) before it shoots, since a
// runner's virtual display starts at one pixel a point.
import CoreGraphics
let display = CGMainDisplayID()
let options = [kCGDisplayShowDuplicateLowResolutionModes: kCFBooleanTrue] as CFDictionary
let modes = (CGDisplayCopyAllDisplayModes(display, options) as? [CGDisplayMode]) ?? []
func line(_ m: CGDisplayMode) -> String { "\(m.width) \(m.height) \(m.pixelWidth) \(m.pixelHeight)" }
if let current = CGDisplayCopyDisplayMode(display) { print("current", line(current)) }
if CommandLine.arguments.dropFirst().first == "best" {
    let tall = modes.filter { $0.pixelHeight == 2 * $0.height }
    guard let best = tall.max(by: { ($0.height, $0.width) < ($1.height, $1.width) }) else {
        print("no HiDPI mode; keeping the current mode")
        exit(0)
    }
    var config: CGDisplayConfigRef?
    CGBeginDisplayConfiguration(&config)
    CGConfigureDisplayWithDisplayMode(config, display, best, nil)
    let result = CGCompleteDisplayConfiguration(config, .permanently)
    print(result == .success ? "set \(line(best))" : "could not set \(line(best)): \(result.rawValue)")
} else {
    for m in modes { print(line(m)) }
}
