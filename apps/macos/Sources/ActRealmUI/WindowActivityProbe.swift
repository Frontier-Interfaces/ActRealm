import AppKit
import Combine
import SwiftUI

enum WindowRenderPolicy {
    static func shouldRender(
        isVisible: Bool,
        isMiniaturized: Bool,
        isOcclusionVisible: Bool
    ) -> Bool {
        isVisible && !isMiniaturized && isOcclusionVisible
    }
}

/// Projects real AppKit window visibility into SwiftUI. App activation is
/// deliberately not part of this signal: a visible ActRealm window must keep
/// its timers and live task projection current while the user works in Codex
/// beside it. Only minimized or genuinely occluded windows pause rendering.
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
                WindowRenderPolicy.shouldRender(
                    isVisible: $0.isVisible,
                    isMiniaturized: $0.isMiniaturized,
                    isOcclusionVisible: $0.occlusionState.contains(.visible)
                )
            } ?? false
            guard active != lastValue else { return }
            lastValue = active
            onChange(active)
        }

        private func observeWindow(_ window: NSWindow?) {
            cancellables.removeAll()
            guard let window else { return }
            Publishers.MergeMany([
                NSWindow.didMiniaturizeNotification,
                NSWindow.didDeminiaturizeNotification,
                NSWindow.didChangeOcclusionStateNotification,
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
