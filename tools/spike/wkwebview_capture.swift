// 検証: WKWebView の画面を撮って VideoToolbox (H.264) で圧縮する。撮れた枚数と圧縮の結果を数える。
// 使い方: capture <秒数> <出力.h264>
import AppKit
import CoreVideo
import VideoToolbox
import WebKit

let args = CommandLine.arguments
let seconds = args.count > 1 ? Double(args[1])! : 20
let outPath = args.count > 2 ? args[2] : "out.h264"
let W = 1280, H = 720, FPS = 30

FileManager.default.createFile(atPath: outPath, contents: nil)
let out = FileHandle(forWritingAtPath: outPath)!
var encodedFrames = 0, encodedBytes = 0, snapshots = 0, snapshotFails = 0, inFlight = 0, skipped = 0

// VideoToolbox: Annex B で書き出す (SPS/PPS はキーフレームごとに付ける)
var session: VTCompressionSession?
VTCompressionSessionCreate(allocator: nil, width: Int32(W), height: Int32(H), codecType: kCMVideoCodecType_H264,
                           encoderSpecification: nil, imageBufferAttributes: nil, compressedDataAllocator: nil,
                           outputCallback: { _, _, status, _, sample in
    guard status == noErr, let sample else { return }
    let start: [UInt8] = [0, 0, 0, 1]
    var data = Data()
    let attach = CMSampleBufferGetSampleAttachmentsArray(sample, createIfNecessary: false) as? [[CFString: Any]]
    let key = !(attach?.first?[kCMSampleAttachmentKey_NotSync] as? Bool ?? false)
    if key, let fmt = CMSampleBufferGetFormatDescription(sample) {
        for i in 0..<2 {
            var p: UnsafePointer<UInt8>?; var n = 0
            CMVideoFormatDescriptionGetH264ParameterSetAtIndex(fmt, parameterSetIndex: i, parameterSetPointerOut: &p, parameterSetSizeOut: &n, parameterSetCountOut: nil, nalUnitHeaderLengthOut: nil)
            data.append(contentsOf: start); data.append(p!, count: n)
        }
    }
    let block = CMSampleBufferGetDataBuffer(sample)!
    var len = 0; var ptr: UnsafeMutablePointer<CChar>?
    CMBlockBufferGetDataPointer(block, atOffset: 0, lengthAtOffsetOut: nil, totalLengthOut: &len, dataPointerOut: &ptr)
    var off = 0
    while off + 4 < len {
        var nal: UInt32 = 0; memcpy(&nal, ptr! + off, 4); nal = CFSwapInt32BigToHost(nal)
        data.append(contentsOf: start)
        data.append(Data(bytes: ptr! + off + 4, count: Int(nal)))
        off += 4 + Int(nal)
    }
    out.write(data); encodedFrames += 1; encodedBytes += data.count
}, refcon: nil, compressionSessionOut: &session)
let s = session!
VTSessionSetProperty(s, key: kVTCompressionPropertyKey_RealTime, value: kCFBooleanTrue)
VTSessionSetProperty(s, key: kVTCompressionPropertyKey_ProfileLevel, value: kVTProfileLevel_H264_Main_AutoLevel)
VTSessionSetProperty(s, key: kVTCompressionPropertyKey_AverageBitRate, value: 3_000_000 as CFNumber)
VTSessionSetProperty(s, key: kVTCompressionPropertyKey_MaxKeyFrameInterval, value: FPS * 2 as CFNumber)
VTSessionSetProperty(s, key: kVTCompressionPropertyKey_AllowFrameReordering, value: kCFBooleanFalse)
VTCompressionSessionPrepareToEncodeFrames(s)

func pixelBuffer(from image: CGImage) -> CVPixelBuffer? {
    var pb: CVPixelBuffer?
    CVPixelBufferCreate(nil, W, H, kCVPixelFormatType_32BGRA, [kCVPixelBufferIOSurfacePropertiesKey: [:]] as CFDictionary, &pb)
    guard let pb else { return nil }
    CVPixelBufferLockBaseAddress(pb, [])
    let ctx = CGContext(data: CVPixelBufferGetBaseAddress(pb), width: W, height: H, bitsPerComponent: 8,
                        bytesPerRow: CVPixelBufferGetBytesPerRow(pb), space: CGColorSpaceCreateDeviceRGB(),
                        bitmapInfo: CGImageAlphaInfo.premultipliedFirst.rawValue | CGBitmapInfo.byteOrder32Little.rawValue)!
    ctx.draw(image, in: CGRect(x: 0, y: 0, width: W, height: H))
    CVPixelBufferUnlockBaseAddress(pb, [])
    return pb
}

let app = NSApplication.shared
app.setActivationPolicy(.accessory)
// 画面の外に置いたウィンドウ (見えないが描画はされる)
let window = NSWindow(contentRect: NSRect(x: -5000, y: -5000, width: W, height: H), styleMask: [.borderless], backing: .buffered, defer: false)
let web = WKWebView(frame: NSRect(x: 0, y: 0, width: W, height: H))
window.contentView = web
window.orderFrontRegardless()
web.load(URLRequest(url: URL(string: "https://eq.fuga.jp/?broadcast=1")!))

let t0 = Date()
var frameNo: Int64 = 0
let cfg = WKSnapshotConfiguration()
cfg.snapshotWidth = NSNumber(value: W)
Timer.scheduledTimer(withTimeInterval: 1.0 / Double(FPS), repeats: true) { timer in
    let elapsed = Date().timeIntervalSince(t0)
    if elapsed > seconds + 3 {
        timer.invalidate()
        VTCompressionSessionCompleteFrames(s, untilPresentationTimeStamp: .invalid)
        out.closeFile()
        let secs = seconds
        print(String(format: "snapshots=%d fails=%d skipped(busy)=%d encoded=%d bytes=%d -> %.1f fps, %.2f Mbps",
                     snapshots, snapshotFails, skipped, encodedFrames, encodedBytes, Double(snapshots) / secs, Double(encodedBytes) * 8 / secs / 1e6))
        exit(0)
    }
    guard elapsed > 3 else { return } // 読み込みを待つ
    if inFlight > 0 { skipped += 1; return }
    inFlight += 1
    let pts = CMTime(value: frameNo, timescale: Int32(FPS)); frameNo += 1
    web.takeSnapshot(with: cfg) { img, _ in
        inFlight -= 1
        guard let img, let cg = img.cgImage(forProposedRect: nil, context: nil, hints: nil), let pb = pixelBuffer(from: cg) else { snapshotFails += 1; return }
        snapshots += 1
        VTCompressionSessionEncodeFrame(s, imageBuffer: pb, presentationTimeStamp: pts, duration: .invalid, frameProperties: nil, sourceFrameRefcon: nil, infoFlagsOut: nil)
    }
}
app.run()
