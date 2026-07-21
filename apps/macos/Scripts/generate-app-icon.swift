#!/usr/bin/env swift

import AppKit
import CoreGraphics
import Foundation

private let canvasSize = 1_024

private func color(
    _ red: CGFloat,
    _ green: CGFloat,
    _ blue: CGFloat,
    _ alpha: CGFloat = 1
) -> CGColor {
    CGColor(
        colorSpace: CGColorSpace(name: CGColorSpace.sRGB)!,
        components: [red / 255, green / 255, blue / 255, alpha]
    )!
}

private func bitmapContext(width: Int, height: Int) -> CGContext {
    let colorSpace = CGColorSpace(name: CGColorSpace.sRGB)!
    return CGContext(
        data: nil,
        width: width,
        height: height,
        bitsPerComponent: 8,
        bytesPerRow: 0,
        space: colorSpace,
        bitmapInfo: CGImageAlphaInfo.premultipliedLast.rawValue
    )!
}

private func renderMaster() -> CGImage {
    let context = bitmapContext(width: canvasSize, height: canvasSize)
    context.setAllowsAntialiasing(true)
    context.setShouldAntialias(true)

    let tileRect = CGRect(x: 96, y: 96, width: 832, height: 832)
    let tile = CGPath(roundedRect: tileRect, cornerWidth: 190, cornerHeight: 190, transform: nil)

    context.saveGState()
    context.setShadow(
        offset: CGSize(width: 0, height: -18),
        blur: 34,
        color: color(16, 34, 94, 0.3)
    )
    context.addPath(tile)
    context.setFillColor(color(48, 85, 205))
    context.fillPath()
    context.restoreGState()

    context.saveGState()
    context.addPath(tile)
    context.clip()
    let baseGradient = CGGradient(
        colorsSpace: CGColorSpace(name: CGColorSpace.sRGB),
        colors: [color(91, 134, 248), color(59, 110, 240), color(43, 82, 207)] as CFArray,
        locations: [0, 0.48, 1]
    )!
    context.drawLinearGradient(
        baseGradient,
        start: CGPoint(x: 260, y: 930),
        end: CGPoint(x: 780, y: 92),
        options: []
    )

    let glowGradient = CGGradient(
        colorsSpace: CGColorSpace(name: CGColorSpace.sRGB),
        colors: [color(255, 255, 255, 0.2), color(255, 255, 255, 0)] as CFArray,
        locations: [0, 1]
    )!
    context.drawLinearGradient(
        glowGradient,
        start: CGPoint(x: 512, y: 920),
        end: CGPoint(x: 512, y: 470),
        options: []
    )
    context.restoreGState()

    context.saveGState()
    context.addPath(tile)
    context.setStrokeColor(color(255, 255, 255, 0.2))
    context.setLineWidth(4)
    context.strokePath()
    context.restoreGState()

    let barWidth: CGFloat = 88
    let gap: CGFloat = 49
    let baseline: CGFloat = 320
    let heights: [CGFloat] = [196, 392, 280]
    let totalWidth = barWidth * 3 + gap * 2
    let startX = (CGFloat(canvasSize) - totalWidth) / 2

    context.saveGState()
    context.setShadow(
        offset: CGSize(width: 0, height: -10),
        blur: 18,
        color: color(18, 35, 94, 0.22)
    )
    for index in heights.indices {
        let rect = CGRect(
            x: startX + CGFloat(index) * (barWidth + gap),
            y: baseline,
            width: barWidth,
            height: heights[index]
        )
        let bar = CGPath(
            roundedRect: rect,
            cornerWidth: barWidth / 2,
            cornerHeight: barWidth / 2,
            transform: nil
        )
        context.addPath(bar)
        context.setFillColor(color(255, 255, 255, 0.97))
        context.fillPath()
    }
    context.restoreGState()

    return context.makeImage()!
}

private func resized(_ image: CGImage, to pixels: Int) -> CGImage {
    let context = bitmapContext(width: pixels, height: pixels)
    context.interpolationQuality = .high
    context.draw(image, in: CGRect(x: 0, y: 0, width: pixels, height: pixels))
    return context.makeImage()!
}

private func writePNG(_ image: CGImage, to url: URL) throws {
    let representation = NSBitmapImageRep(cgImage: image)
    representation.size = NSSize(width: image.width, height: image.height)
    guard let data = representation.representation(using: .png, properties: [:]) else {
        throw CocoaError(.fileWriteUnknown)
    }
    try data.write(to: url)
}

let resourcesDirectory = CommandLine.arguments.dropFirst().first.map {
    URL(fileURLWithPath: $0, isDirectory: true)
} ?? URL(fileURLWithPath: FileManager.default.currentDirectoryPath, isDirectory: true)
let fileManager = FileManager.default
try fileManager.createDirectory(at: resourcesDirectory, withIntermediateDirectories: true)

let temporaryRoot = fileManager.temporaryDirectory
    .appendingPathComponent("ActRealmIcon-\(UUID().uuidString)", isDirectory: true)
let iconsetDirectory = temporaryRoot.appendingPathComponent("ActRealm.iconset", isDirectory: true)
try fileManager.createDirectory(at: iconsetDirectory, withIntermediateDirectories: true)
defer { try? fileManager.removeItem(at: temporaryRoot) }

let master = renderMaster()
try writePNG(master, to: resourcesDirectory.appendingPathComponent("ActRealmIcon.png"))

let variants: [(String, Int)] = [
    ("icon_16x16.png", 16),
    ("icon_16x16@2x.png", 32),
    ("icon_32x32.png", 32),
    ("icon_32x32@2x.png", 64),
    ("icon_128x128.png", 128),
    ("icon_128x128@2x.png", 256),
    ("icon_256x256.png", 256),
    ("icon_256x256@2x.png", 512),
    ("icon_512x512.png", 512),
    ("icon_512x512@2x.png", 1_024),
]
for (name, pixels) in variants {
    try writePNG(resized(master, to: pixels), to: iconsetDirectory.appendingPathComponent(name))
}

let iconutil = Process()
iconutil.executableURL = URL(fileURLWithPath: "/usr/bin/iconutil")
iconutil.arguments = [
    "-c", "icns",
    iconsetDirectory.path,
    "-o", resourcesDirectory.appendingPathComponent("ActRealm.icns").path,
]
try iconutil.run()
iconutil.waitUntilExit()
guard iconutil.terminationStatus == 0 else {
    throw CocoaError(.fileWriteUnknown)
}

print("Generated \(resourcesDirectory.appendingPathComponent("ActRealm.icns").path)")
