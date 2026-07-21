import AppKit
import AVFoundation
import ActRealmKit
import ImageIO
import SwiftUI

/// Shared renderer for static images, animated GIFs, and muted looping video
/// backgrounds. Every source is a local file copied into Application Support.
struct AppThemeMediaView: View {
    let url: URL
    let kind: ThemeBackgroundKind

    var body: some View {
        Group {
            switch kind {
            case .image:
                AppThemeImage(url: url)
            case .animatedImage:
                AnimatedGIFView(url: url)
            case .video:
                LoopingVideoView(url: url)
            }
        }
        .clipped()
    }
}

private struct AnimatedGIFView: NSViewRepresentable {
    let url: URL

    func makeNSView(context: Context) -> GIFPlayerView {
        let view = GIFPlayerView()
        view.configure(url: url)
        return view
    }

    func updateNSView(_ view: GIFPlayerView, context: Context) {
        view.configure(url: url)
    }

    static func dismantleNSView(_ view: GIFPlayerView, coordinator: ()) {
        view.stop()
    }
}

@MainActor
private final class GIFPlayerView: NSView {
    private var source: CGImageSource?
    private var frameDurations: [TimeInterval] = []
    private var frameIndex = 0
    private var timer: Timer?
    private var loadedURL: URL?

    override init(frame frameRect: NSRect) {
        super.init(frame: frameRect)
        wantsLayer = true
        layer?.backgroundColor = NSColor.clear.cgColor
    }

    convenience init() {
        self.init(frame: .zero)
    }

    required init?(coder: NSCoder) {
        super.init(coder: coder)
    }

    func configure(url: URL) {
        guard loadedURL != url else { return }
        stop()
        loadedURL = url
        source = CGImageSourceCreateWithURL(url as CFURL, nil)
        guard let source else { return }
        let count = CGImageSourceGetCount(source)
        frameDurations = (0..<count).map { Self.frameDuration(source: source, index: $0) }
        frameIndex = 0
        needsDisplay = true
        scheduleNextFrame()
    }

    func stop() {
        timer?.invalidate()
        timer = nil
    }

    override func viewDidMoveToWindow() {
        super.viewDidMoveToWindow()
        if window == nil {
            stop()
        } else if timer == nil, source != nil {
            scheduleNextFrame()
        }
    }

    override func draw(_ dirtyRect: NSRect) {
        super.draw(dirtyRect)
        guard let source,
              let frame = CGImageSourceCreateImageAtIndex(source, frameIndex, nil),
              let context = NSGraphicsContext.current?.cgContext,
              bounds.width > 0,
              bounds.height > 0
        else { return }

        let imageSize = CGSize(width: frame.width, height: frame.height)
        let scale = max(bounds.width / imageSize.width, bounds.height / imageSize.height)
        let targetSize = CGSize(width: imageSize.width * scale, height: imageSize.height * scale)
        let target = CGRect(
            x: bounds.midX - targetSize.width / 2,
            y: bounds.midY - targetSize.height / 2,
            width: targetSize.width,
            height: targetSize.height
        )
        context.saveGState()
        context.clip(to: bounds)
        context.interpolationQuality = .high
        context.draw(frame, in: target)
        context.restoreGState()
    }

    @objc private func advanceFrame() {
        guard !frameDurations.isEmpty else { return }
        frameIndex = (frameIndex + 1) % frameDurations.count
        needsDisplay = true
        scheduleNextFrame()
    }

    private func scheduleNextFrame() {
        guard window != nil || loadedURL != nil,
              frameDurations.indices.contains(frameIndex)
        else { return }
        timer?.invalidate()
        let next = Timer(
            timeInterval: frameDurations[frameIndex],
            target: self,
            selector: #selector(advanceFrame),
            userInfo: nil,
            repeats: false
        )
        timer = next
        RunLoop.main.add(next, forMode: .common)
    }

    private static func frameDuration(source: CGImageSource, index: Int) -> TimeInterval {
        guard let properties = CGImageSourceCopyPropertiesAtIndex(source, index, nil)
            as? [CFString: Any],
              let gif = properties[kCGImagePropertyGIFDictionary] as? [CFString: Any]
        else { return 0.1 }
        let unclamped = (gif[kCGImagePropertyGIFUnclampedDelayTime] as? NSNumber)?.doubleValue
        let clamped = (gif[kCGImagePropertyGIFDelayTime] as? NSNumber)?.doubleValue
        return max(0.02, unclamped ?? clamped ?? 0.1)
    }
}

private struct LoopingVideoView: NSViewRepresentable {
    let url: URL

    func makeNSView(context: Context) -> LoopingVideoPlayerView {
        let view = LoopingVideoPlayerView()
        view.configure(url: url)
        return view
    }

    func updateNSView(_ view: LoopingVideoPlayerView, context: Context) {
        view.configure(url: url)
    }

    static func dismantleNSView(_ view: LoopingVideoPlayerView, coordinator: ()) {
        view.stop()
    }
}

@MainActor
private final class LoopingVideoPlayerView: NSView {
    private let playerLayer = AVPlayerLayer()
    private var player: AVQueuePlayer?
    private var looper: AVPlayerLooper?
    private var loadedURL: URL?

    override init(frame frameRect: NSRect) {
        super.init(frame: frameRect)
        wantsLayer = true
        playerLayer.videoGravity = .resizeAspectFill
        layer?.addSublayer(playerLayer)
    }

    convenience init() {
        self.init(frame: .zero)
    }

    required init?(coder: NSCoder) {
        super.init(coder: coder)
        wantsLayer = true
        playerLayer.videoGravity = .resizeAspectFill
        layer?.addSublayer(playerLayer)
    }

    override func layout() {
        super.layout()
        playerLayer.frame = bounds
    }

    override func viewDidMoveToWindow() {
        super.viewDidMoveToWindow()
        window == nil ? player?.pause() : player?.play()
    }

    func configure(url: URL) {
        guard loadedURL != url else { return }
        stop()
        loadedURL = url
        let queue = AVQueuePlayer()
        queue.isMuted = true
        queue.actionAtItemEnd = .none
        let item = AVPlayerItem(url: url)
        player = queue
        looper = AVPlayerLooper(player: queue, templateItem: item)
        playerLayer.player = queue
        queue.play()
    }

    func stop() {
        player?.pause()
        playerLayer.player = nil
        looper = nil
        player = nil
    }
}
