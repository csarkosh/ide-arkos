// Records one window, by its window server id, to a QuickTime file: the window alone, whatever
// lies over it, without its shadow or the pointer, through ScreenCaptureKit's recording output
// (macOS 15 and later). arkos-shoot compiles this with swiftc and runs it for a storyboard's
// `[[recording]]`; it prints "recording" once frames flow, and stops, finishing the file, on
// SIGINT or SIGTERM or after --max-secs.
//
//   record <window id> <out.mov> [--fps 30] [--max-secs 120]
//
// The window's frames come at most `fps` a second and only when something on it changes, so the
// file's frame timing is uneven; scripts/shoot/loop.py re-times it at a constant rate.
import AVFoundation
import Foundation
import ScreenCaptureKit

func fail(_ message: String) -> Never {
    FileHandle.standardError.write((message + "\n").data(using: .utf8)!)
    exit(2)
}

var args = Array(CommandLine.arguments.dropFirst())
var fps = 30
var maxSecs = 120.0
var positional: [String] = []
while !args.isEmpty {
    let a = args.removeFirst()
    switch a {
    case "--fps":
        guard !args.isEmpty, let n = Int(args.removeFirst()), (1...60).contains(n) else { fail("--fps takes 1 to 60") }
        fps = n
    case "--max-secs":
        guard !args.isEmpty, let s = Double(args.removeFirst()), s > 0 else { fail("--max-secs takes seconds") }
        maxSecs = s
    default:
        positional.append(a)
    }
}
guard positional.count == 2, let windowId = UInt32(positional[0]) else {
    fail("usage: record <window id> <out.mov> [--fps 30] [--max-secs 120]")
}
let outURL = URL(fileURLWithPath: positional[1])
try? FileManager.default.removeItem(at: outURL)

final class Recorder: NSObject, SCRecordingOutputDelegate, SCStreamDelegate {
    var stream: SCStream?
    var stopping = false

    func recordingOutputDidStartRecording(_ recordingOutput: SCRecordingOutput) {
        print("recording")
        fflush(stdout)
    }

    func recordingOutputDidFinishRecording(_ recordingOutput: SCRecordingOutput) {
        print("finished \(outURL.path)")
        fflush(stdout)
        exit(0)
    }

    func recordingOutput(_ recordingOutput: SCRecordingOutput, didFailWithError error: Error) {
        fail("recording failed: \(error.localizedDescription)")
    }

    func stream(_ stream: SCStream, didStopWithError error: Error) {
        fail("the stream stopped: \(error.localizedDescription)")
    }

    func stop() {
        guard !stopping, let stream else { return }
        stopping = true
        Task {
            do {
                try await stream.stopCapture()
            } catch {
                fail("stopCapture: \(error.localizedDescription)")
            }
            // recordingOutputDidFinishRecording exits; if it never comes, do not hang the shoot.
            try? await Task.sleep(for: .seconds(10))
            fail("the recording did not finish within 10 s of stopping")
        }
    }
}

let recorder = Recorder()

func onSignal(_ sig: Int32) {
    signal(sig, SIG_IGN)
    let source = DispatchSource.makeSignalSource(signal: sig, queue: .main)
    source.setEventHandler { recorder.stop() }
    source.resume()
    // Kept alive for the process's life.
    _ = Unmanaged.passRetained(source)
}
onSignal(SIGINT)
onSignal(SIGTERM)

Task {
    do {
        let content = try await SCShareableContent.excludingDesktopWindows(false, onScreenWindowsOnly: false)
        guard let window = content.windows.first(where: { $0.windowID == windowId }) else {
            fail("the window server lists no window \(windowId)")
        }
        let scale = NSScreen.main?.backingScaleFactor ?? 2
        let config = SCStreamConfiguration()
        config.width = Int(window.frame.width * scale)
        config.height = Int(window.frame.height * scale)
        config.minimumFrameInterval = CMTime(value: 1, timescale: CMTimeScale(fps))
        config.showsCursor = false
        config.ignoreShadowsSingleWindow = true
        config.shouldBeOpaque = true
        config.queueDepth = 6
        let filter = SCContentFilter(desktopIndependentWindow: window)
        let stream = SCStream(filter: filter, configuration: config, delegate: recorder)
        let outputConfig = SCRecordingOutputConfiguration()
        outputConfig.outputURL = outURL
        outputConfig.outputFileType = .mov
        outputConfig.videoCodecType = .h264
        let output = SCRecordingOutput(configuration: outputConfig, delegate: recorder)
        try stream.addRecordingOutput(output)
        recorder.stream = stream
        try await stream.startCapture()
        try? await Task.sleep(for: .seconds(maxSecs))
        recorder.stop()
    } catch {
        fail("could not record window \(windowId): \(error.localizedDescription)")
    }
}
// ScreenCaptureKit puts a status item in the menu bar while it records, which AppKit insists on
// making on the main thread: the main thread stays the main queue's (dispatchMain would hand the
// queue to a worker and the recording would crash the moment it started).
NSApplication.shared.setActivationPolicy(.prohibited)
RunLoop.main.run()
