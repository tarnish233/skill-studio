import AppKit
import SwiftUI
import WebKit

// C ABI only: Rust owns the callback, AppKit owns the views. Every entry point
// below is invoked on Tauri's main thread.
private typealias EventCallback = @convention(c) (UnsafePointer<CChar>) -> Void
private var controllerKey: UInt8 = 0

private struct ToolRect: Decodable {
    var x: Double
    var y: Double
    var width: Double
    var height: Double
}

private struct ToolPacket: Decodable {
    var session: String
    var revision: UInt64
    var owner: String
    var query: String
    var edit: UInt64
    var placeholder: String
    var createLabel: String
    var createDisabledReason: String
    var showAdd: Bool
    var visible: Bool
    var viewportWidth: Double
    var search: ToolRect
    var add: ToolRect
}

@MainActor
private final class ToolModel: ObservableObject {
    @Published var placeholder = ""
    @Published var createLabel = ""
    @Published var disabledReason = ""
    var owner = ""
    var query = ""
    var edit: UInt64 = 0
    var onSearch: (String) -> Void = { _ in }
    var onCreate: () -> Void = {}
    weak var field: NSSearchField?
}

private struct NativeSearch: NSViewRepresentable {
    @ObservedObject var model: ToolModel
    var usesGlass = false

    func makeNSView(context: Context) -> NSSearchField {
        let field = NSSearchField()
        field.stringValue = model.query
        field.controlSize = .regular
        field.font = .systemFont(ofSize: 13)
        // Keep the native bezel geometry so the field editor reserves space
        // for the search and clear buttons, but let glass supply the background.
        if usesGlass {
            field.drawsBackground = false
            field.backgroundColor = .clear
        }
        field.sendsSearchStringImmediately = true
        field.sendsWholeSearchString = false
        field.delegate = context.coordinator
        field.target = context.coordinator
        field.action = #selector(Coordinator.searchChanged(_:))
        field.focusRingType = .exterior
        model.field = field
        return field
    }

    func updateNSView(_ field: NSSearchField, context: Context) {
        field.placeholderString = model.placeholder
        field.setAccessibilityLabel(model.placeholder)
    }

    func makeCoordinator() -> Coordinator { Coordinator(model) }

    @MainActor
    final class Coordinator: NSObject, NSSearchFieldDelegate {
        let model: ToolModel
        init(_ model: ToolModel) { self.model = model }

        func controlTextDidChange(_ notification: Notification) {
            guard let field = notification.object as? NSSearchField else { return }
            searchChanged(field)
        }

        @objc func searchChanged(_ field: NSSearchField) {
            // Keep candidate composition inside AppKit until the IME commits it.
            guard !((field.currentEditor() as? NSTextView)?.hasMarkedText() ?? false) else { return }
            model.onSearch(field.stringValue)
        }
    }
}

private struct NativeSearchControl: View {
    @ObservedObject var model: ToolModel

    @ViewBuilder var body: some View {
        #if compiler(>=6.2)
        if #available(macOS 26.0, *) {
            NativeSearch(model: model, usesGlass: true)
                .frame(height: 24)
                .glassEffect(.regular.interactive(), in: .capsule)
                .frame(maxWidth: .infinity, maxHeight: .infinity)
        } else {
            NativeSearch(model: model)
        }
        #else
        NativeSearch(model: model)
        #endif
    }
}

private struct NativeAddButton: View {
    @ObservedObject var model: ToolModel

    private var button: some View {
        Button(action: model.onCreate) {
            Image(systemName: "plus")
                .font(.system(size: 16, weight: .medium))
                .frame(width: 16, height: 16)
        }
        .accessibilityLabel(model.createLabel)
        .help(model.disabledReason.isEmpty ? model.createLabel : model.disabledReason)
        .disabled(!model.disabledReason.isEmpty)
    }

    @ViewBuilder var body: some View {
        #if compiler(>=6.2)
        if #available(macOS 26.0, *) {
            button
                .buttonStyle(.glassProminent)
                .buttonBorderShape(.circle)
                .tint(.blue)
                .controlSize(.regular)
        } else {
            legacyButton
        }
        #else
        legacyButton
        #endif
    }

    private var legacyButton: some View {
        button
            .buttonStyle(.borderedProminent)
            .tint(.blue)
            .controlSize(.regular)
            .clipShape(Circle())
    }
}

@MainActor
private final class ToolController: NSObject {
    let session: String
    let callback: EventCallback
    let model = ToolModel()
    let search: NSHostingView<AnyView>
    let add: NSHostingView<AnyView>
    weak var parent: NSView?
    private var revision: UInt64 = 0

    init(parent: NSView, session: String, callback: @escaping EventCallback) {
        self.parent = parent
        self.session = session
        self.callback = callback
        search = NSHostingView(rootView: AnyView(NativeSearchControl(model: model).ignoresSafeArea()))
        add = NSHostingView(rootView: AnyView(NativeAddButton(model: model).ignoresSafeArea()))
        super.init()
        for view in [search as NSView, add as NSView] {
            view.isHidden = true
            view.autoresizingMask = [.minXMargin, .minYMargin]
            parent.addSubview(view, positioned: .above, relativeTo: nil)
        }
        if #available(macOS 13.0, *) {
            search.sizingOptions = []
            add.sizingOptions = []
        }
        // Geometry already includes the overlay titlebar; applying SwiftUI's
        // window safe area again would push these toolbar controls downward.
        if #available(macOS 13.3, *) {
            search.safeAreaRegions = []
            add.safeAreaRegions = []
        }
        model.onSearch = { [weak self] value in
            guard let self, !self.search.isHidden, value != self.model.query else { return }
            self.model.query = value
            self.model.edit += 1
            self.emit("search", value: value)
        }
        model.onCreate = { [weak self] in
            guard let self, !self.add.isHidden, self.model.disabledReason.isEmpty else { return }
            self.focusWebView()
            self.emit("create", value: "")
        }
    }

    func detach() {
        releaseSearchFocus()
        search.removeFromSuperview()
        add.removeFromSuperview()
    }

    private func emit(_ kind: String, value: String) {
        let event: [String: Any] = [
            "session": session, "owner": model.owner,
            "kind": kind, "value": value, "edit": model.edit,
        ]
        guard let data = try? JSONSerialization.data(withJSONObject: event),
              let json = String(data: data, encoding: .utf8) else { return }
        json.withCString { callback($0) }
    }

    private func focusWebView() {
        func find(_ view: NSView) -> WKWebView? {
            if let webView = view as? WKWebView { return webView }
            return view.subviews.lazy.compactMap { find($0) }.first
        }
        if let parent, let webView = find(parent) {
            parent.window?.makeFirstResponder(webView)
        }
    }

    private func releaseSearchFocus() {
        guard let field = model.field, let editor = field.currentEditor(),
              parent?.window?.firstResponder === editor else { return }
        focusWebView()
    }

    func update(_ packet: ToolPacket) {
        guard packet.session == session, packet.revision > revision,
              let parent else { return }
        revision = packet.revision
        let changedOwner = model.owner != packet.owner
        if changedOwner || !packet.visible { releaseSearchFocus() }
        model.owner = packet.owner
        model.placeholder = packet.placeholder
        model.createLabel = packet.createLabel
        model.disabledReason = packet.createDisabledReason
        // An older JS acknowledgment must never overwrite a newer native edit.
        if changedOwner || packet.edit >= model.edit {
            model.edit = packet.edit
            model.query = packet.query
            if let field = model.field, field.stringValue != packet.query {
                let composing = (field.currentEditor() as? NSTextView)?.hasMarkedText() ?? false
                if changedOwner || !composing { field.stringValue = packet.query }
            }
        }
        let scale = parent.bounds.width / max(packet.viewportWidth, 1)
        for (view, rect) in [(search as NSView, packet.search), (add as NSView, packet.add)] {
            view.frame = NSRect(
                x: rect.x * scale,
                y: parent.isFlipped ? rect.y * scale : parent.bounds.height - (rect.y + rect.height) * scale,
                width: rect.width * scale, height: rect.height * scale
            )
        }
        search.isHidden = !packet.visible
        add.isHidden = !packet.visible || !packet.showAdd
    }
}

@_cdecl("ss_page_tools_init")
@MainActor
public func pageToolsInit(
    _ parentPointer: UnsafeMutableRawPointer,
    _ sessionPointer: UnsafePointer<CChar>,
    _ callback: @escaping @convention(c) (UnsafePointer<CChar>) -> Void
) {
    let parent = Unmanaged<NSView>.fromOpaque(parentPointer).takeUnretainedValue()
    (objc_getAssociatedObject(parent, &controllerKey) as? ToolController)?.detach()
    let controller = ToolController(
        parent: parent, session: String(cString: sessionPointer), callback: callback
    )
    objc_setAssociatedObject(parent, &controllerKey, controller, .OBJC_ASSOCIATION_RETAIN_NONATOMIC)
}

@_cdecl("ss_page_tools_update")
@MainActor
public func pageToolsUpdate(_ parentPointer: UnsafeMutableRawPointer, _ json: UnsafePointer<CChar>) {
    let parent = Unmanaged<NSView>.fromOpaque(parentPointer).takeUnretainedValue()
    guard let controller = objc_getAssociatedObject(parent, &controllerKey) as? ToolController,
          let data = String(cString: json).data(using: .utf8),
          let packet = try? JSONDecoder().decode(ToolPacket.self, from: data) else { return }
    controller.update(packet)
}

@_cdecl("ss_page_tools_destroy")
@MainActor
public func pageToolsDestroy(_ parentPointer: UnsafeMutableRawPointer, _ session: UnsafePointer<CChar>) {
    let parent = Unmanaged<NSView>.fromOpaque(parentPointer).takeUnretainedValue()
    guard let controller = objc_getAssociatedObject(parent, &controllerKey) as? ToolController,
          controller.session == String(cString: session) else { return }
    controller.detach()
    objc_setAssociatedObject(parent, &controllerKey, nil, .OBJC_ASSOCIATION_RETAIN_NONATOMIC)
}
