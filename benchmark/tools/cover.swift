// Covers every screen with an opaque window above normal windows for a while, as another app's
// window would cover the benchmark's: `swift tools/cover.swift <seconds>`. A covered macOS window
// is not presented, so the platform requests no frames for it.
import AppKit

let seconds = Double(CommandLine.arguments.dropFirst().first ?? "4") ?? 4
let app = NSApplication.shared
app.setActivationPolicy(.accessory)
var windows: [NSWindow] = []
for screen in NSScreen.screens {
    let window = NSWindow(
        contentRect: screen.frame, styleMask: .borderless, backing: .buffered, defer: false)
    window.level = .floating
    window.backgroundColor = .black
    window.isOpaque = true
    window.orderFrontRegardless()
    windows.append(window)
}
// Tell the caller the screens are covered.
print("covered", terminator: "\n")
fflush(stdout)
DispatchQueue.main.asyncAfter(deadline: .now() + seconds) {
    for window in windows { window.orderOut(nil) }
    DispatchQueue.main.asyncAfter(deadline: .now() + 0.5) { exit(0) }
}
app.run()
