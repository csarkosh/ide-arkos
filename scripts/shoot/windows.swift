// The window server's list of ordinary windows (layers 0 to 3: normal and floating, which the
// shoot's window is; wider than 100 points), one a line:
// id, owner pid, x, y, width, height (points, top-left origin), on screen (1 or 0), owner.
// arkos-shoot runs it (`swift scripts/shoot/windows.swift`) to find its window and the Chrome
// window Arkos docked, and captures them by id, in front or not.
import CoreGraphics
let list = CGWindowListCopyWindowInfo([.optionAll], kCGNullWindowID) as! [[String: Any]]
for w in list where ((w["kCGWindowLayer"] as? Int) ?? 99) <= 3 {
    let b = w["kCGWindowBounds"] as! [String: Any]
    guard (b["Width"] as! Double) > 100 else { continue }
    let on = (w["kCGWindowIsOnscreen"] as? Bool) ?? false
    print(w["kCGWindowNumber"] as! Int, w["kCGWindowOwnerPID"] as! Int, b["X"]!, b["Y"]!, b["Width"]!, b["Height"]!, on ? 1 : 0, w["kCGWindowOwnerName"] as? String ?? "?")
}
