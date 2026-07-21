import AppKit
import ActRealmKit
import ActRealmUI
import SwiftUI

/// Offscreen renderer: captures screens that support SwiftUI ImageRenderer
/// with demo data so they can be verified without screen recording
/// permissions. NavigationSplitView-based settings are checked in the hosted
/// packaged app because SwiftUI does not render that container offscreen.

@MainActor
func writePNG(_ image: NSImage, to path: String, scale: CGFloat) {
    guard let tiff = image.tiffRepresentation,
          let rep = NSBitmapImageRep(data: tiff),
          let data = rep.representation(using: .png, properties: [:])
    else {
        FileHandle.standardError.write(Data("failed to encode \(path)\n".utf8))
        return
    }
    try? data.write(to: URL(fileURLWithPath: path))
    print("wrote \(path) (\(Int(image.size.width * scale))x\(Int(image.size.height * scale))px)")
}

@MainActor
func render(_ view: some View, size: CGSize?, to path: String, scale: CGFloat = 2) {
    let renderer = ImageRenderer(content: AnyView(view.environment(\.snapshotRendering, true)))
    renderer.scale = scale
    if let size {
        renderer.proposedSize = ProposedViewSize(size)
    }
    guard let image = renderer.nsImage else {
        FileHandle.standardError.write(Data("failed to render \(path)\n".utf8))
        return
    }
    writePNG(image, to: path, scale: scale)
}

/// Colorful desktop stand-in matching the prototype's backdrop, so glass
/// translucency reads in the capture.
struct Backdrop<Content: View>: View {
    let dark: Bool
    @ViewBuilder var content: Content

    var body: some View {
        ZStack {
            if dark {
                LinearGradient(
                    colors: [
                        Color(red: 0.04, green: 0.055, blue: 0.11),
                        Color(red: 0.075, green: 0.063, blue: 0.153),
                        Color(red: 0.04, green: 0.07, blue: 0.125),
                    ],
                    startPoint: .topLeading, endPoint: .bottomTrailing
                )
                RadialGradient(
                    colors: [Color(red: 0.27, green: 0.47, blue: 1).opacity(0.32), .clear],
                    center: .init(x: 0.2, y: 0.05), startRadius: 0, endRadius: 500
                )
                RadialGradient(
                    colors: [Color(red: 0.59, green: 0.34, blue: 1).opacity(0.26), .clear],
                    center: .init(x: 0.9, y: 0.5), startRadius: 0, endRadius: 420
                )
            } else {
                LinearGradient(
                    colors: [
                        Color(red: 0.918, green: 0.941, blue: 1),
                        Color(red: 0.961, green: 0.941, blue: 1),
                        Color(red: 0.902, green: 0.953, blue: 0.953),
                    ],
                    startPoint: .topLeading, endPoint: .bottomTrailing
                )
                RadialGradient(
                    colors: [Color(red: 0.478, green: 0.635, blue: 1).opacity(0.5), .clear],
                    center: .init(x: 0.15, y: 0.1), startRadius: 0, endRadius: 640
                )
                RadialGradient(
                    colors: [Color(red: 0.77, green: 0.61, blue: 1).opacity(0.42), .clear],
                    center: .init(x: 0.86, y: 0.24), startRadius: 0, endRadius: 520
                )
                RadialGradient(
                    colors: [Color(red: 0.47, green: 0.84, blue: 0.78).opacity(0.35), .clear],
                    center: .init(x: 0.55, y: 1.0), startRadius: 0, endRadius: 600
                )
            }
            content
        }
    }
}

let outDir = CommandLine.arguments.count > 1
    ? CommandLine.arguments[1]
    : FileManager.default.currentDirectoryPath + "/snapshots"
try? FileManager.default.createDirectory(atPath: outDir, withIntermediateDirectories: true)

Task { @MainActor in
    NSApplication.shared.setActivationPolicy(.prohibited)
    let model = AppModel(demo: true)
    model.start()

    // Main window — light and dark.
    for dark in [false, true] {
        let name = dark ? "main-dark" : "main-light"
        render(
            Backdrop(dark: dark) {
                MainWindowView()
                    .environmentObject(model)
                    .frame(width: 1536, height: 820)
                    .clipShape(RoundedRectangle(cornerRadius: 22, style: .continuous))
                    .padding(40)
            }
            .environment(\.colorScheme, dark ? .dark : .light),
            size: CGSize(width: 1616, height: 900),
            to: "\(outDir)/\(name).png"
        )
    }

    model.expandedTaskId = "claude-quota-fix"
    model.pinnedSessionId = "claude-quota-fix"
    render(
        Backdrop(dark: false) {
            MainWindowView()
                .environmentObject(model)
                .frame(width: 1536, height: 820)
                .clipShape(RoundedRectangle(cornerRadius: 22, style: .continuous))
                .padding(40)
        },
        size: CGSize(width: 1616, height: 900),
        to: "\(outDir)/main-expanded-light.png"
    )

    model.outboxPageIndex = 1
    render(
        Backdrop(dark: false) {
            MainWindowView()
                .environmentObject(model)
                .frame(width: 1536, height: 980)
                .clipShape(RoundedRectangle(cornerRadius: 22, style: .continuous))
                .padding(40)
        },
        size: CGSize(width: 1616, height: 1060),
        to: "\(outDir)/interactive-question-light.png"
    )
    model.outboxPageIndex = 0

    render(
        Backdrop(dark: false) {
            MainWindowView(initialPage: .agentSetup)
                .environmentObject(model)
                .frame(width: 1536, height: 980)
                .clipShape(RoundedRectangle(cornerRadius: 22, style: .continuous))
                .padding(40)
        },
        size: CGSize(width: 1616, height: 1060),
        to: "\(outDir)/agent-setup-light.png"
    )

    // Foreground scheduling management page. The taller viewport captures the
    // full reference layout in one QA artifact while the shipped window keeps
    // the page scrollable at normal desktop sizes.
    render(
        Backdrop(dark: false) {
            MainWindowView(initialPage: .foregroundScheduling)
                .environmentObject(model)
                .frame(width: 1536, height: 1660)
                .clipShape(RoundedRectangle(cornerRadius: 22, style: .continuous))
                .padding(40)
        },
        size: CGSize(width: 1616, height: 1740),
        to: "\(outDir)/foreground-scheduling-light.png"
    )

    // HUD capsule.
    for dark in [false, true] {
        let name = dark ? "hud-dark" : "hud-light"
        render(
            Backdrop(dark: dark) {
                HUDCapsuleView()
                    .environmentObject(model)
                    .padding(30)
            }
            .environment(\.colorScheme, dark ? .dark : .light),
            size: nil,
            to: "\(outDir)/\(name).png"
        )
    }

    model.previewForegroundScheduling()
    render(
        Backdrop(dark: false) {
            HUDCapsuleView()
                .environmentObject(model)
                .padding(30)
        },
        size: nil,
        to: "\(outDir)/hud-foreground-reminder-light.png"
    )
    model.keepForegroundTaskInActRealmWorkspace()

    // Menu bar popover.
    for dark in [false, true] {
        let name = dark ? "popover-dark" : "popover-light"
        render(
            Backdrop(dark: dark) {
                MenuBarPopoverView()
                    .environmentObject(model)
                    .background(dark
                        ? Color(red: 0.1, green: 0.11, blue: 0.14).opacity(0.92)
                        : Color.white.opacity(0.72))
                    .clipShape(RoundedRectangle(cornerRadius: 18, style: .continuous))
                    .padding(24)
            }
            .environment(\.colorScheme, dark ? .dark : .light),
            size: nil,
            to: "\(outDir)/\(name).png"
        )
    }

    // Runtime monitor sheet.
    render(
        Backdrop(dark: false) {
            RuntimeMonitorView()
                .environmentObject(model)
                .padding(30)
        },
        size: nil,
        to: "\(outDir)/runtime-monitor-light.png"
    )

    exit(0)
}

RunLoop.main.run()
