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
// ScreenCaptureKit delivers (one only when the window changes), taken as 4:2:0 BT.709 (NV12,
// what the H.264 holds in the end, at 1.5 bytes a pixel rather than BGRA's 4: a Retina window
// at 30 fps otherwise outgrows a runner's disk), is LZ4-compressed and appended
// to a scratch file in the temporary folder with its time; afterwards the frames are replayed at a
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
// The scratch file goes in the temporary folder, not beside the output: a recording ended
// before it encoded would otherwise leave gigabytes of frames among the pictures (and in the
// workflow's artifact).
let scratchPath = NSTemporaryDirectory() + "arkos-shoot-record-\(getpid()).frames"
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
        // NV12: the luma plane, then the interleaved chroma plane at half the height.
        guard CVPixelBufferGetPlaneCount(pixels) == 2, let luma = CVPixelBufferGetBaseAddressOfPlane(pixels, 0), let chroma = CVPixelBufferGetBaseAddressOfPlane(pixels, 1) else { return }
        var frame = Data(capacity: width * height * 3 / 2)
        let lumaStride = CVPixelBufferGetBytesPerRowOfPlane(pixels, 0)
        for y in 0..<height {
            frame.append(luma.advanced(by: y * lumaStride).assumingMemoryBound(to: UInt8.self), count: width)
        }
        let chromaStride = CVPixelBufferGetBytesPerRowOfPlane(pixels, 1)
        for y in 0..<(height / 2) {
            frame.append(chroma.advanced(by: y * chromaStride).assumingMemoryBound(to: UInt8.self), count: width)
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
            // A full disk ends the capture; what was taken is still encoded, and the loop's cut
            // says plainly if that is too short.
            FileHandle.standardError.write("record: could not write a frame after \(String(format: "%.1f", now.timeIntervalSince(started!))) s (\(error.localizedDescription)); stopping here\n".data(using: .utf8)!)
            stop()
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
                       "-f", "rawvideo", "-pix_fmt", "nv12", "-s", "\(width)x\(height)", "-r", "\(fps)", "-i", "-",
                       "-colorspace", "bt709", "-color_primaries", "bt709", "-color_trc", "iec61966-2-1", "-color_range", "tv",
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
        // What a 4:2:0 frame costs on disk, against what the temporary folder has: a note up
        // front beats a capture that stops short.
        let perFrame = Double(recorder.width * recorder.height * 3 / 2)
        let expected = perFrame * Double(fps) * maxSecs * 0.6
        if let free = (try? FileManager.default.attributesOfFileSystem(forPath: NSTemporaryDirectory()))?[.systemFreeSize] as? Double {
            FileHandle.standardError.write("record: \(Int(free / 1e9)) GB free for up to \(Int(expected / 1e9)) GB of frames\n".data(using: .utf8)!)
            if free < expected { FileHandle.standardError.write("record: that may not be enough for the whole recording\n".data(using: .utf8)!) }
        }
        let config = SCStreamConfiguration()
        config.width = recorder.width
        config.height = recorder.height
        config.pixelFormat = kCVPixelFormatType_420YpCbCr8BiPlanarVideoRange
        config.colorMatrix = CGDisplayStream.yCbCrMatrix_ITU_R_709_2
        config.colorSpaceName = CGColorSpace.sRGB
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
