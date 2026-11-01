// Records one window, by its window server id, to an H.264 file: the window alone, whatever
// lies over it, without its shadow or the pointer, through ScreenCaptureKit (macOS 14 and
// later). arkos-shoot compiles this with swiftc and runs it for a storyboard's `[[recording]]`;
// it prints "recording" once frames flow, and stops on SIGINT or SIGTERM or after --max-secs,
// then encodes and prints "finished".
//
//   record <window id> <out.mp4> [--fps 30] [--max-secs 120]
//
// Capture and encoding are kept apart, since a GitHub macOS runner has no video encoder of its
// own and its software encoder cannot keep up with live frames: while it records, each frame
// ScreenCaptureKit delivers (one only when the window changes) is LZ4-compressed and appended
// to a scratch file beside the output with its time; afterwards the frames are replayed at a
// constant `fps` (the latest frame at each tick) into ffmpeg, which encodes them in software:
// x264's ultrafast preset at a low CRF, a near-transparent intermediate that a runner's cores
// manage at a Retina window's size (loop.py encodes the loop properly from it).
// The capture is at the window's display's density: two pixels a point on a Retina Mac or on
// the shoot's virtual display, one on a bare runner.
import AppKit
import CoreMedia
import CoreVideo
import Foundation
import ScreenCaptureKit

func fail(_ message: String) -> Never {
    FileHandle.standardError.write(("record: " + message + "\n").data(using: .utf8)!)
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
let scratchPath = outPath + ".frames"
try? FileManager.default.removeItem(atPath: outPath)
try? FileManager.default.removeItem(atPath: scratchPath)

final class Recorder: NSObject, SCStreamOutput, SCStreamDelegate {
    let queue = DispatchQueue(label: "record.frames")
    var stream: SCStream?
    var scratch: FileHandle?
    /// Each captured frame: seconds since the first, its offset in the scratch file and its
    /// compressed size.
    var index: [(time: Double, offset: UInt64, size: Int)] = []
    var started: Date?
    var width = 0
    var height = 0
    var stopping = false

    func stream(_ stream: SCStream, didOutputSampleBuffer sampleBuffer: CMSampleBuffer, of type: SCStreamOutputType) {
        guard !stopping, type == .screen, sampleBuffer.isValid,
              let attachments = CMSampleBufferGetSampleAttachmentsArray(sampleBuffer, createIfNecessary: false) as? [[SCStreamFrameInfo: Any]],
              let status = attachments.first?[.status] as? Int, status == SCFrameStatus.complete.rawValue,
              let pixels = CMSampleBufferGetImageBuffer(sampleBuffer), let scratch else { return }
        CVPixelBufferLockBaseAddress(pixels, .readOnly)
        defer { CVPixelBufferUnlockBaseAddress(pixels, .readOnly) }
        guard let base = CVPixelBufferGetBaseAddress(pixels) else { return }
        let stride = CVPixelBufferGetBytesPerRow(pixels)
        let rowBytes = width * 4
        var frame = Data(capacity: rowBytes * height)
        for y in 0..<height {
            frame.append(base.advanced(by: y * stride).assumingMemoryBound(to: UInt8.self), count: rowBytes)
        }
        guard let packed = try? (frame as NSData).compressed(using: .lz4) as Data else { return }
        let now = Date()
        if started == nil {
            started = now
            print("recording")
            fflush(stdout)
        }
        do {
            let offset = try scratch.offset()
            try scratch.write(contentsOf: packed)
            index.append((now.timeIntervalSince(started!), offset, packed.count))
        } catch {
            fail("could not write a frame: \(error.localizedDescription)")
        }
    }

    func stream(_ stream: SCStream, didStopWithError error: Error) {
        fail("the stream stopped: \(error.localizedDescription)")
    }

    func stop() {
        queue.async {
            guard !self.stopping else { return }
            self.stopping = true
            if let stream = self.stream {
                let done = DispatchSemaphore(value: 0)
                stream.stopCapture { _ in done.signal() }
                _ = done.wait(timeout: .now() + 5)
            }
            guard let started = self.started, !self.index.isEmpty else { fail("no frame of the window arrived") }
            let length = Date().timeIntervalSince(started)
            try? self.scratch?.close()
            self.encode(length: length)
            try? FileManager.default.removeItem(atPath: scratchPath)
            print("finished \(outPath): \(self.index.count) frames captured over \(String(format: "%.1f", length)) s")
            fflush(stdout)
            exit(0)
        }
    }

    /// The captured frames replayed at a constant rate into ffmpeg: at each tick the latest
    /// frame taken by then, so a still window holds and a moving one keeps its timing.
    func encode(length: Double) {
        let p = Process()
        p.executableURL = URL(fileURLWithPath: "/usr/bin/env")
        p.arguments = ["ffmpeg", "-hide_banner", "-loglevel", "error", "-nostats", "-y",
                       "-f", "rawvideo", "-pix_fmt", "bgra", "-s", "\(width)x\(height)", "-r", "\(fps)", "-i", "-",
                       "-c:v", "libx264", "-preset", "ultrafast", "-crf", "14", "-pix_fmt", "yuv420p", outPath]
        let pipe = Pipe()
        p.standardInput = pipe
        p.standardOutput = FileHandle.standardError
        p.standardError = FileHandle.standardError
        do { try p.run() } catch { fail("could not start ffmpeg: \(error.localizedDescription)") }
        let sink = pipe.fileHandleForWriting
        guard let reader = FileHandle(forReadingAtPath: scratchPath) else { fail("the scratch file vanished") }
        var at = 0
        var loaded = -1
        var frame = Data()
        let ticks = Int((length * Double(fps)).rounded(.down))
        let encodeStart = Date()
        for tick in 0..<max(ticks, 1) {
            if tick > 0 && tick % 300 == 0 {
                FileHandle.standardError.write("record: encoded \(tick) of \(ticks) frames in \(Int(Date().timeIntervalSince(encodeStart))) s\n".data(using: .utf8)!)
            }
            let t = Double(tick) / Double(fps)
            while at + 1 < index.count && index[at + 1].time <= t { at += 1 }
            if at != loaded {
                do {
                    try reader.seek(toOffset: index[at].offset)
                    guard let packed = try reader.read(upToCount: index[at].size), let raw = try? (packed as NSData).decompressed(using: .lz4) as Data else { fail("could not read frame \(at) back") }
                    frame = raw
                } catch {
                    fail("could not read frame \(at) back: \(error.localizedDescription)")
                }
                loaded = at
            }
            do { try sink.write(contentsOf: frame) } catch { fail("ffmpeg stopped taking frames: \(error.localizedDescription)") }
        }
        try? sink.close()
        p.waitUntilExit()
        if p.terminationStatus != 0 { fail("ffmpeg failed (status \(p.terminationStatus))") }
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
        // The display the window is on says how many pixels a point it draws; even sides, since
        // the encoder's 4:2:0 wants them.
        let centre = CGPoint(x: window.frame.midX, y: window.frame.midY)
        let display = content.displays.first { $0.frame.contains(centre) } ?? content.displays.first
        var scale = 1.0
        if let display, let mode = CGDisplayCopyDisplayMode(display.displayID), mode.width > 0 {
            scale = Double(mode.pixelWidth) / Double(mode.width)
        }
        recorder.width = Int((window.frame.width * scale).rounded()) & ~1
        recorder.height = Int((window.frame.height * scale).rounded()) & ~1
        FileHandle.standardError.write("record: window \(Int(window.frame.width))x\(Int(window.frame.height)) points at \(scale)x, \(recorder.width)x\(recorder.height) pixels\n".data(using: .utf8)!)
        guard FileManager.default.createFile(atPath: scratchPath, contents: nil), let scratch = FileHandle(forWritingAtPath: scratchPath) else {
            fail("could not make \(scratchPath)")
        }
        recorder.scratch = scratch
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
        try await stream.startCapture()
        let check = min(10.0, maxSecs)
        try? await Task.sleep(for: .seconds(check))
        if recorder.started == nil { fail("no frame of window \(windowId) arrived in \(Int(check)) s") }
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
