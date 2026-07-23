import AppKit
import Combine
import SwiftUI

/// Projects the real AppKit window visibility into SwiftUI. `scenePhase`
/// alone remains active for some minimized/occluded macOS windows, which lets
/// TimelineView continue consuming CPU in the background.
struct WindowActivityProbe: NSViewRepresentable {
    let onChange: (Bool) -> Void

    func makeCoordinator() -> Coordinator {
        Coordinator(onChange: onChange)
    }

    func makeNSView(context: Context) -> ProbeView {
        let view = ProbeView()
        view.onWindowChange = { [weak coordinator = context.coordinator] window in
            coordinator?.attach(to: window)
        }
        return view
    }

    func updateNSView(_ nsView: ProbeView, context: Context) {
        context.coordinator.onChange = onChange
        context.coordinator.evaluate()
    }

    @MainActor
    final class Coordinator {
        var onChange: (Bool) -> Void
        private weak var window: NSWindow?
        private var cancellables: Set<AnyCancellable> = []
        private var lastValue: Bool?

        init(onChange: @escaping (Bool) -> Void) {
            self.onChange = onChange
            observeApplication()
        }

        func attach(to window: NSWindow?) {
            guard self.window !== window else {
                evaluate()
                return
            }
            self.window = window
            observeWindow(window)
            evaluate()
        }

        func evaluate() {
            let active = window.map {
                NSApplication.shared.isActive
                    && $0.isVisible
                    && !$0.isMiniaturized
                    && $0.occlusionState.contains(.visible)
            } ?? false
            guard active != lastValue else { return }
            lastValue = active
            onChange(active)
        }

        private func observeApplication() {
            Publishers.Merge(
                NotificationCenter.default.publisher(for: NSApplication.didBecomeActiveNotification),
                NotificationCenter.default.publisher(for: NSApplication.didResignActiveNotification)
            )
            .receive(on: RunLoop.main)
            .sink { [weak self] _ in self?.evaluate() }
            .store(in: &cancellables)
        }

        private func observeWindow(_ window: NSWindow?) {
            cancellables.removeAll()
            observeApplication()
            guard let window else { return }
            Publishers.MergeMany([
                NSWindow.didMiniaturizeNotification,
                NSWindow.didDeminiaturizeNotification,
                NSWindow.didChangeOcclusionStateNotification,
                NSWindow.didBecomeKeyNotification,
                NSWindow.didResignKeyNotification,
            ].map {
                NotificationCenter.default.publisher(for: $0, object: window).eraseToAnyPublisher()
            })
            .receive(on: RunLoop.main)
            .sink { [weak self] _ in self?.evaluate() }
            .store(in: &cancellables)
        }
    }

    final class ProbeView: NSView {
        var onWindowChange: ((NSWindow?) -> Void)?

        override func viewDidMoveToWindow() {
            super.viewDidMoveToWindow()
            onWindowChange?(window)
        }
    }
}
