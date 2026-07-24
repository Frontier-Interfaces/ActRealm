#!/usr/bin/env swift

import AppKit
import CoreGraphics
import Foundation
import ImageIO

private let canvasSize = 1_024

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

private func loadMaster(from url: URL) throws -> CGImage {
    guard
        let source = CGImageSourceCreateWithURL(url as CFURL, nil),
        let image = CGImageSourceCreateImageAtIndex(source, 0, nil)
    else {
        throw CocoaError(.fileReadCorruptFile)
    }
    guard image.width == canvasSize, image.height == canvasSize else {
        fputs(
            "error: source icon must be exactly \(canvasSize)x\(canvasSize) pixels\n",
            stderr
        )
        throw CocoaError(.fileReadUnsupportedScheme)
    }
    return image
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
let outputMasterURL = resourcesDirectory.appendingPathComponent("ActRealmIcon.png")
let sourceURL = CommandLine.arguments.dropFirst(2).first.map {
    URL(fileURLWithPath: $0)
} ?? outputMasterURL

guard fileManager.fileExists(atPath: sourceURL.path) else {
    fputs(
        """
        error: source icon not found at \(sourceURL.path)
        usage: generate-app-icon.swift [resources-directory] [source-1024px-png]

        """,
        stderr
    )
    throw CocoaError(.fileNoSuchFile)
}

let master = try loadMaster(from: sourceURL)
if sourceURL.standardizedFileURL != outputMasterURL.standardizedFileURL {
    try Data(contentsOf: sourceURL).write(to: outputMasterURL, options: .atomic)
}

let temporaryRoot = fileManager.temporaryDirectory
    .appendingPathComponent("ActRealmIcon-\(UUID().uuidString)", isDirectory: true)
let iconsetDirectory = temporaryRoot.appendingPathComponent("ActRealm.iconset", isDirectory: true)
try fileManager.createDirectory(at: iconsetDirectory, withIntermediateDirectories: true)
defer { try? fileManager.removeItem(at: temporaryRoot) }

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

print("Updated \(outputMasterURL.path)")
print("Generated \(resourcesDirectory.appendingPathComponent("ActRealm.icns").path)")
