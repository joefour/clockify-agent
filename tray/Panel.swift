// The tray's panel in plain AppKit, in claude-usage's Swiss style (its DESIGN.md:
// Helvetica Neue, ink on paper, Swiss red for Send and alarms, square corners,
// rules not boxes). AppKit, not SwiftUI, keeps that framework out of a menu bar
// process. The pure choices sit up top so the SELFTEST block in main.swift can
// check them. Layout helpers are claude-usage's tray/Panel.swift, trimmed.
import AppKit

// MARK: - Pure logic

enum Phase: Equatable {
    case idle
    case running(since: Date)
    case done(TimeInterval)
    case failed(String)
    case stopped
}

enum Tone: Equatable { case ink, red, muted }

/// The header's meta line.
func metaText(_ p: Phase, now: Date) -> String {
    switch p {
    case .idle: "Sonnet 5.5 · bypass"
    case .running(let since): "Running · \(Int(now.timeIntervalSince(since))) s"
    case .done(let d): "Done in \(Int(d.rounded())) s"
    case .failed(let why): "Failed · \(why)"
    case .stopped: "Stopped"
    }
}

/// The reply's status word; nil before the first run.
func statusWord(_ p: Phase) -> (String, Tone)? {
    switch p {
    case .idle: nil
    case .running: ("WORKING", .ink)
    case .done: ("DONE", .ink)
    case .failed: ("FAILED", .red)
    case .stopped: ("STOPPED", .muted)
    }
}

/// How a run ended: Stop wins; then the result event decides; without one, the exit does.
func phaseAfter(exit: Exit, stopped: Bool, events: [Event], duration: TimeInterval) -> Phase {
    if stopped { return .stopped }
    for case .result(_, let isError) in events.reversed() { return isError ? .failed("error") : .done(duration) }
    switch exit {
    case .code(let c): return .failed("exit \(c)")
    case .signal(let s): return .failed("signal \(s)")
    }
}

/// The prompt on one line, cut at 40 characters, for the reply's heading.
func promptTitle(_ s: String) -> String {
    let t = s.split(whereSeparator: \.isWhitespace).joined(separator: " ")
    return t.count > 40 ? t.prefix(40) + "…" : t
}

// MARK: - Model

/// The panel's state. Tray sets it; PanelController shows it while the panel is open.
final class PanelModel {
    var phase = Phase.idle { didSet { changed() } }
    var asked = ""                                       // the run's prompt
    var events: [Event] = [] { didSet { replyChanged() } }
    var trimmed = false                                  // older events were dropped (64 KB cap)
    var stderrTail: [String] = []                        // shown when a run fails without a result
    var repoError: String? { didSet { changed() } }
    var procsOpen = false { didSet { changed() } }
    var claude: [ProcStat]?                              // the run's process tree; Tray calls statsChanged after sampling
    var tray = ""
    var cap: CGFloat = 400                               // tallest the reply may be; Tray sets it from the screen
    // Set by PanelController.
    var changed: () -> Void = {}
    var replyChanged: () -> Void = {}
    var statsChanged: () -> Void = {}
    // Set by Tray.
    var onSend: (String) -> Void = { _ in }
    var onStop: () -> Void = {}
    var onLog: () -> Void = {}
    var onQuit: () -> Void = {}
    var onResize: () -> Void = {}
}
