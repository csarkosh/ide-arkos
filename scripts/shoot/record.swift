// Records one window, by its window server id, to an H.264 file: the window alone, whatever
// lies over it, without its shadow or the pointer, through ScreenCaptureKit (macOS 14 and
// later). The frames go raw to ffmpeg, which encodes them in software: a GitHub macOS runner
// has no video encoder of its own, so ScreenCaptureKit's recording output fails there on its
// first frame. arkos-shoot compiles this with swiftc and runs it for a storyboard's
// `[[recording]]`; it prints "recording" once frames flow, and stops, finishing the file, on
// SIGINT or SIGTERM or after --max-secs.
//
//   record <window id> <out.mp4> [--fps 30] [--max-secs 120]
//
// ScreenCaptureKit delivers a frame only when the window changes, so a timer writes the latest
// frame `fps` times a second and the file keeps a constant rate. The capture is one pixel a
// point, which is the runner's density and what the page shows.
import AppKit
import CoreMedia
import CoreVideo
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
    fail("usage: record <window id> <out.mp4> [--fps 30] [--max-secs 120]")
}
let outPath = positional[1]
try? FileManager.default.removeItem(atPath: outPath)

final class Recorder: NSObject, SCStreamOutput, SCStreamDelegate {
    let queue = DispatchQueue(label: "record.frames")
    var stream: SCStream?
    var encoder: Process?
    var sink: FileHandle?
    var timer: DispatchSourceTimer?
    var latest: Data?
    var width = 0
    var height = 0
    var announced = false
    var stopping = false

    /// ffmpeg reading raw BGRA frames on its standard input, writing the file.
    func startEncoder() throws {
        let p = Process()
        p.executableURL = URL(fileURLWithPath: "/usr/bin/env")
        p.arguments = ["ffmpeg", "-hide_banner", "-loglevel", "error", "-y",
                       "-f", "rawvideo", "-pix_fmt", "bgra", "-s", "\(width)x\(height)", "-r", "\(fps)", "-i", "-",
                       "-c:v", "libx264", "-preset", "veryfast", "-crf", "18", "-pix_fmt", "yuv420p", outPath]
        let pipe = Pipe()
        p.standardInput = pipe
        p.standardOutput = FileHandle.standardError
        p.standardError = FileHandle.standardError
        try p.run()
        encoder = p
        sink = pipe.fileHandleForWriting
    }

    func stream(_ stream: SCStream, didOutputSampleBuffer sampleBuffer: CMSampleBuffer, of type: SCStreamOutputType) {
        guard type == .screen, sampleBuffer.isValid,
              let attachments = CMSampleBufferGetSampleAttachmentsArray(sampleBuffer, createIfNecessary: false) as? [[SCStreamFrameInfo: Any]],
              let status = attachments.first?[.status] as? Int, status == SCFrameStatus.complete.rawValue,
              let pixels = CMSampleBufferGetImageBuffer(sampleBuffer) else { return }
        CVPixelBufferLockBaseAddress(pixels, .readOnly)
        defer { CVPixelBufferUnlockBaseAddress(pixels, .readOnly) }
        guard let base = CVPixelBufferGetBaseAddress(pixels) else { return }
        let stride = CVPixelBufferGetBytesPerRow(pixels)
        let rowBytes = width * 4
        var frame = Data(capacity: rowBytes * height)
        for y in 0..<height {
            frame.append(base.advanced(by: y * stride).assumingMemoryBound(to: UInt8.self), count: rowBytes)
        }
        latest = frame
    }

    /// The latest frame to ffmpeg, `fps` times a second, from the first frame on.
    func tick() {
        guard let frame = latest, let sink, !stopping else { return }
        do {
            try sink.write(contentsOf: frame)
        } catch {
            fail("ffmpeg stopped taking frames: \(error.localizedDescription)")
        }
        if !announced {
            announced = true
            print("recording")
            fflush(stdout)
        }
    }

    func stream(_ stream: SCStream, didStopWithError error: Error) {
        fail("the stream stopped: \(error.localizedDescription)")
    }

    func stop() {
        queue.async {
            guard !self.stopping else { return }
            self.stopping = true
            self.timer?.cancel()
            if let stream = self.stream {
                let done = DispatchSemaphore(value: 0)
                stream.stopCapture { _ in done.signal() }
                _ = done.wait(timeout: .now() + 5)
            }
            try? self.sink?.close()
            if let encoder = self.encoder {
                encoder.waitUntilExit()
                if encoder.terminationStatus != 0 { fail("ffmpeg failed (status \(encoder.terminationStatus))") }
            }
            if !self.announced { fail("no frame of the window arrived") }
            print("finished \(outPath)")
            fflush(stdout)
            exit(0)
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
        // Even sides: the encoder's 4:2:0 wants them.
        recorder.width = Int(window.frame.width) & ~1
        recorder.height = Int(window.frame.height) & ~1
        let config = SCStreamConfiguration()
        config.width = recorder.width
        config.height = recorder.height
        config.pixelFormat = kCVPixelFormatType_32BGRA
        config.minimumFrameInterval = CMTime(value: 1, timescale: CMTimeScale(fps))
        config.showsCursor = false
        config.ignoreShadowsSingleWindow = true
        config.shouldBeOpaque = true
        config.queueDepth = 6
        let filter = SCContentFilter(desktopIndependentWindow: window)
        let stream = SCStream(filter: filter, configuration: config, delegate: recorder)
        try stream.addStreamOutput(recorder, type: .screen, sampleHandlerQueue: recorder.queue)
        recorder.stream = stream
        try recorder.startEncoder()
        try await stream.startCapture()
        let timer = DispatchSource.makeTimerSource(queue: recorder.queue)
        timer.schedule(deadline: .now(), repeating: 1.0 / Double(fps), leeway: .milliseconds(2))
        timer.setEventHandler { recorder.tick() }
        timer.resume()
        recorder.timer = timer
        let check = min(10.0, maxSecs)
        try? await Task.sleep(for: .seconds(check))
        if !recorder.announced { fail("no frame of window \(windowId) arrived in \(Int(check)) s") }
        if maxSecs > check { try? await Task.sleep(for: .seconds(maxSecs - check)) }
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
