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
    var cap: CGFloat = 400                               // tallest the whole panel may be; Tray sets it from the screen
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

// MARK: - Palette

struct Pal {
    let paper, ink, muted, redInk: NSColor
    func text(_ t: Tone) -> NSColor { t == .red ? redInk : t == .muted ? muted : ink }
}

extension NSColor {
    convenience init(hex: UInt32) { self.init(srgbRed: CGFloat(hex >> 16 & 255) / 255, green: CGFloat(hex >> 8 & 255) / 255, blue: CGFloat(hex & 255) / 255, alpha: 1) }
}

let swissRed = NSColor(hex: 0xda291c)

/// claude-usage's DESIGN.md values.
func pal(dark: Bool) -> Pal {
    dark ? Pal(paper: NSColor(hex: 0x0d0d0d), ink: NSColor(hex: 0xf2f2f2), muted: NSColor(hex: 0xa3a3a3), redInk: NSColor(hex: 0xff6a5c))
         : Pal(paper: NSColor(hex: 0xffffff), ink: NSColor(hex: 0x111111), muted: NSColor(hex: 0x5c5c5c), redInk: NSColor(hex: 0xda291c))
}

// MARK: - Pieces

enum W { case r, m, b }

/// Helvetica Neue at a fixed size, with tabular digits.
func hvFont(_ size: CGFloat, _ w: W = .r) -> NSFont {
    let f = NSFont(name: w == .b ? "HelveticaNeue-Bold" : w == .m ? "HelveticaNeue-Medium" : "HelveticaNeue", size: size) ?? .systemFont(ofSize: size)
    let tnum: [NSFontDescriptor.FeatureKey: Int] = [.typeIdentifier: kNumberSpacingType, .selectorIdentifier: kMonospacedNumbersSelector]
    return NSFont(descriptor: f.fontDescriptor.addingAttributes([.featureSettings: [tnum]]), size: size) ?? f
}

/// A text label. `em` is letter-spacing in em; `wrap` is the width it wraps at, else it stays on one line, cut per `cut`.
func label(_ s: String, _ size: CGFloat, _ w: W = .r, em: CGFloat = 0, _ color: NSColor, wrap: CGFloat? = nil,
           align: NSTextAlignment = .left, cut: NSLineBreakMode = .byTruncatingTail) -> NSTextField {
    let ps = NSMutableParagraphStyle()
    ps.alignment = align
    ps.lineBreakMode = wrap == nil ? cut : .byWordWrapping
    let a = NSAttributedString(string: s, attributes: [.font: hvFont(size, w), .kern: em * size, .foregroundColor: color, .paragraphStyle: ps])
    guard let wrap else { return NSTextField(labelWithAttributedString: a) }
    let f = NSTextField(wrappingLabelWithString: "")
    f.isSelectable = false
    f.attributedStringValue = a
    f.preferredMaxLayoutWidth = wrap
    return f
}

/// A regular-weight label.
func label(_ s: String, _ size: CGFloat, _ color: NSColor, wrap: CGFloat? = nil, align: NSTextAlignment = .left) -> NSTextField {
    label(s, size, .r, color, wrap: wrap, align: align)
}

func recolor(_ f: NSTextField, _ c: NSColor) {
    let a = NSMutableAttributedString(attributedString: f.attributedStringValue)
    a.addAttribute(.foregroundColor, value: c, range: NSRange(location: 0, length: a.length))
    f.attributedStringValue = a
}

/// Flipped, so a scroll view shows it from the top.
final class Stack: NSStackView {
    override var isFlipped: Bool { true }
}

/// A column; each item stretches to its width less the insets.
func vstack(_ items: [NSView], _ spacing: CGFloat = 0, inset: NSEdgeInsets = NSEdgeInsetsZero) -> Stack {
    let s = Stack(views: items)
    s.orientation = .vertical
    s.alignment = .leading
    s.spacing = spacing
    s.edgeInsets = inset
    for v in items { v.widthAnchor.constraint(equalTo: s.widthAnchor, constant: -inset.left - inset.right).isActive = true }
    return s
}

/// A row, centered or top-aligned; a `spacer()` or a `stretch`ed item takes the room left.
func hstack(_ items: [NSView], _ spacing: CGFloat = 0, top: Bool = false) -> Stack {
    let s = Stack(views: items)
    s.orientation = .horizontal
    s.alignment = top ? .top : .centerY
    s.distribution = .fill
    s.spacing = spacing
    return s
}

func stretch<V: NSView>(_ v: V) -> V {
    v.setContentHuggingPriority(.init(1), for: .horizontal)
    return v
}

func spacer() -> NSView { stretch(NSView()) }

func fixed<V: NSView>(_ v: V, w: CGFloat? = nil, h: CGFloat? = nil) -> V {
    v.translatesAutoresizingMaskIntoConstraints = false
    if let w { v.widthAnchor.constraint(equalToConstant: w).isActive = true }
    if let h { v.heightAnchor.constraint(equalToConstant: h).isActive = true }
    return v
}

/// A plain block of color: rules, squares.
func block(_ c: NSColor, w: CGFloat? = nil, h: CGFloat? = nil) -> NSView {
    let v = NSView()
    v.wantsLayer = true
    v.layer?.backgroundColor = c.cgColor
    return fixed(v, w: w, h: h)
}

func edges(_ t: CGFloat, _ l: CGFloat, _ b: CGFloat, _ r: CGFloat) -> NSEdgeInsets { NSEdgeInsets(top: t, left: l, bottom: b, right: r) }

/// Sets a layer property without Core Animation's implicit fade.
func still(_ f: () -> Void) {
    CATransaction.begin()
    CATransaction.setDisableActions(true)
    f()
    CATransaction.commit()
}

/// Turns `l` about its center every `period` s, unless Reduce Motion.
func turn(_ l: CALayer, _ period: Double) {
    guard !NSWorkspace.shared.accessibilityDisplayShouldReduceMotion else { return }
    let a = CABasicAnimation(keyPath: "transform.rotation.z")
    a.byValue = -2 * Double.pi     // y-up layers: negative turns clockwise
    a.duration = period
    a.repeatCount = .infinity
    l.add(a, forKey: "turn")
}

/// A fixed-size view drawn with shape sublayers.
final class Arcs: NSView {
    init(_ size: CGFloat) {
        super.init(frame: NSRect(x: 0, y: 0, width: size, height: size))
        wantsLayer = true
        _ = fixed(self, w: size, h: size)
    }

    required init?(coder: NSCoder) { fatalError() }

    /// A ring of radius `r` around the center, from 12 o'clock clockwise to `to` (0...1).
    func arc(_ r: CGFloat, _ lw: CGFloat, _ c: NSColor, to: Double = 1) -> CAShapeLayer {
        let path = CGMutablePath()
        path.addArc(center: CGPoint(x: bounds.midX, y: bounds.midY), radius: r, startAngle: .pi / 2, endAngle: .pi / 2 - 2 * .pi, clockwise: true)
        let l = shape(path, lw, c)
        l.strokeEnd = to
        return l
    }

    func shape(_ path: CGPath, _ lw: CGFloat, _ c: NSColor) -> CAShapeLayer {
        let l = CAShapeLayer()
        l.frame = bounds
        l.path = path
        l.fillColor = nil
        l.strokeColor = c.cgColor
        l.lineWidth = lw
        l.contentsScale = NSScreen.main?.backingScaleFactor ?? 2
        layer?.addSublayer(l)
        return l
    }
}

func spinner(_ size: CGFloat, _ c: NSColor) -> (NSView, CAShapeLayer) {
    let v = Arcs(size)
    let ring = v.arc((size - 2) / 2, 2, c, to: 0.75)
    turn(ring, 0.7)
    return (v, ring)
}

/// The mark: an ink ring with a red hand at 12 and 3 o'clock, in a 32-unit box.
func clockMark(_ size: CGFloat, _ p: Pal) -> NSView {
    let v = Arcs(size), k = size / 32
    _ = v.arc(12 * k, 5 * k, p.ink)
    let hand = CGMutablePath()
    hand.move(to: CGPoint(x: 16 * k, y: 23 * k))       // layer space is y-up
    hand.addLine(to: CGPoint(x: 16 * k, y: 16 * k))
    hand.addLine(to: CGPoint(x: 21 * k, y: 16 * k))
    v.shape(hand, 3.5 * k, p.redInk).lineCap = .square
    v.setAccessibilityElement(false)
    return v
}

/// A borderless button made of its own views; `paint` restyles it when it turns hot (hover or press) or back.
final class Press: NSButton {
    var paint: ((Press, Bool) -> Void)? { didSet { repaint() } }
    override var isEnabled: Bool { didSet { repaint() } }
    private let content: NSView, pad: (CGFloat, CGFloat), minHeight: CGFloat, act: () -> Void
    private var hover = false, down = false
    private lazy var fit = content.fittingSize

    /// `pad` is (leading, trailing). `fill` stretches `content` across; otherwise it sits in the middle.
    init(_ content: NSView, pad: (CGFloat, CGFloat) = (0, 0), minHeight: CGFloat = 0, fill: Bool = false, _ act: @escaping () -> Void) {
        self.content = content
        self.pad = pad
        self.minHeight = minHeight
        self.act = act
        super.init(frame: .zero)
        title = ""
        isBordered = false
        wantsLayer = true
        target = self
        action = #selector(fire)
        translatesAutoresizingMaskIntoConstraints = false
        content.translatesAutoresizingMaskIntoConstraints = false
        addSubview(content)
        NSLayoutConstraint.activate([
            content.centerYAnchor.constraint(equalTo: centerYAnchor),
            content.topAnchor.constraint(greaterThanOrEqualTo: topAnchor),
        ] + (fill ? [
            content.leadingAnchor.constraint(equalTo: leadingAnchor, constant: pad.0),
            content.trailingAnchor.constraint(equalTo: trailingAnchor, constant: -pad.1),
        ] : [
            content.centerXAnchor.constraint(equalTo: centerXAnchor, constant: (pad.0 - pad.1) / 2),
            content.leadingAnchor.constraint(greaterThanOrEqualTo: leadingAnchor, constant: pad.0),
        ]))
        addTrackingArea(NSTrackingArea(rect: .zero, options: [.mouseEnteredAndExited, .activeAlways, .inVisibleRect], owner: self))
    }

    required init?(coder: NSCoder) { fatalError() }

    override var intrinsicContentSize: NSSize { NSSize(width: fit.width + pad.0 + pad.1, height: max(minHeight, fit.height)) }
    override var focusRingMaskBounds: NSRect { bounds }
    override func drawFocusRingMask() { bounds.fill() }
    override func accessibilityChildren() -> [Any]? { nil }
    override func mouseEntered(with e: NSEvent) { hover = true; repaint() }
    override func mouseExited(with e: NSEvent) { hover = false; repaint() }

    override func mouseDown(with e: NSEvent) {
        down = true
        repaint()
        super.mouseDown(with: e)    // tracks until the mouse is up and fires the action
        down = false
        repaint()
    }

    @objc private func fire() { act() }
    private func repaint() { paint?(self, isEnabled && (hover || down)) }
}

enum BtnKind { case outline, red }

/// A boxed button (the design's .btn): 2 pt border, ink fill while hot.
func btn(_ kind: BtnKind, _ p: Pal, _ title: String, size: CGFloat = 13, h: CGFloat = 36, pad: CGFloat = 14,
         spin: Bool = false, gap: CGFloat = 6, enabled: Bool = true, _ act: @escaping () -> Void) -> Press {
    let l = label(title, size, .b, p.ink)
    let s = spin ? spinner(12, p.ink) : nil
    let b = Press(hstack([s?.0, l].compactMap { $0 }, gap), pad: (pad, pad), minHeight: h, act)
    b.layer?.borderWidth = 2
    b.isEnabled = enabled
    b.alphaValue = enabled ? 1 : 0.4
    b.paint = { b, hot in
        let (bg, fg, bd): (NSColor, NSColor, NSColor) = switch (kind, hot) {
        case (.outline, false): (.clear, p.ink, p.ink)
        case (.red, false): (swissRed, .white, swissRed)
        default: (p.ink, p.paper, p.ink)
        }
        b.layer?.backgroundColor = bg.cgColor
        b.layer?.borderColor = bd.cgColor
        recolor(l, fg)
        still { s?.1.strokeColor = fg.cgColor }
    }
    b.setAccessibilityLabel(title)
    return b
}

/// A text-only button (the design's .txt): red ink while hot.
func txtBtn(_ p: Pal, _ title: String, _ w: W, color: NSColor? = nil, pad: (CGFloat, CGFloat), key: String = "", _ act: @escaping () -> Void) -> Press {
    let l = label(title, 13, w, color ?? p.ink)
    let b = Press(l, pad: pad, minHeight: 36, act)
    b.paint = { _, hot in recolor(l, hot ? p.redInk : color ?? p.ink) }
    b.keyEquivalent = key
    b.keyEquivalentModifierMask = .command
    b.setAccessibilityLabel(title)
    return b
}

// MARK: - Panel

/// The panel's content. A state change rebuilds it; reply text and stats update in
/// place. The prompt and reply text views are made once and moved into each rebuild,
/// so typed text survives.
final class PanelController: NSViewController, NSTextViewDelegate {
    let m: PanelModel
    private var queued = false, replyQueued = false
    private let promptScroll = NSTextView.scrollableTextView(), replyScroll = NSTextView.scrollableTextView()
    private var prompt: NSTextView { promptScroll.documentView as! NSTextView }
    private var reply: NSTextView { replyScroll.documentView as! NSTextView }
    private let promptBox = NSView()      // the prompt's 2 pt border: NSScrollView keeps no border on its own layer
    private lazy var replyHeight = replyScroll.heightAnchor.constraint(equalToConstant: 18)
    private var col: NSView?
    private var meta: NSTextField?, claudeStat: NSTextField?, trayStat: NSTextField?, procRows: Stack?
    private var shownPhase = Phase.idle
    private var p = pal(dark: false)

    init(_ m: PanelModel) {
        self.m = m
        super.init(nibName: nil, bundle: nil)
        m.changed = { [unowned self] in
            guard !queued else { return }
            queued = true
            DispatchQueue.main.async { [unowned self] in
                if queued, view.window?.isVisible == true { render() }
                queued = false
            }
        }
        // Batched: at most one reply update per 100 ms while the panel shows.
        m.replyChanged = { [unowned self] in
            guard !replyQueued else { return }
            replyQueued = true
            DispatchQueue.main.asyncAfter(deadline: .now() + 0.1) { [unowned self] in
                if replyQueued, view.window?.isVisible == true { flushReply() }
                replyQueued = false
            }
        }
        m.statsChanged = { [unowned self] in updateStats() }
    }

    required init?(coder: NSCoder) { fatalError() }

    override func loadView() {
        let r = Root()
        r.wantsLayer = true
        r.onAppearance = { [unowned self] in if view.window?.isVisible == true { render() } }   // hidden: the next open renders
        view = r
        for s in [promptScroll, replyScroll] {
            s.drawsBackground = false
            s.borderType = .noBorder
            s.autohidesScrollers = true
            s.translatesAutoresizingMaskIntoConstraints = false
        }
        promptBox.wantsLayer = true
        promptBox.layer?.borderWidth = 2
        promptBox.translatesAutoresizingMaskIntoConstraints = false
        promptBox.addSubview(promptScroll)
        NSLayoutConstraint.activate([
            promptBox.heightAnchor.constraint(equalToConstant: 76),
            promptScroll.topAnchor.constraint(equalTo: promptBox.topAnchor, constant: 2),
            promptScroll.bottomAnchor.constraint(equalTo: promptBox.bottomAnchor, constant: -2),
            promptScroll.leadingAnchor.constraint(equalTo: promptBox.leadingAnchor, constant: 2),
            promptScroll.trailingAnchor.constraint(equalTo: promptBox.trailingAnchor, constant: -2),
        ])
        prompt.delegate = self
        prompt.isRichText = false
        prompt.allowsUndo = true
        prompt.drawsBackground = false
        prompt.textContainerInset = NSSize(width: 6, height: 6)
        prompt.isAutomaticQuoteSubstitutionEnabled = false     // card URLs and times stay as typed
        prompt.isAutomaticDashSubstitutionEnabled = false
        prompt.isAutomaticTextReplacementEnabled = false
        prompt.isContinuousSpellCheckingEnabled = false
        prompt.setAccessibilityLabel("Prompt")
        reply.isEditable = false
        reply.isSelectable = true
        reply.drawsBackground = false
        reply.textContainerInset = .zero
        reply.textContainer?.lineFragmentPadding = 0
        reply.setAccessibilityLabel("Reply")
        replyHeight.isActive = true
    }

    func focusPrompt() { view.window?.makeFirstResponder(prompt) }

    func fittingSize() -> NSSize { col?.fittingSize ?? .zero }

    /// Esc in the prompt closes the panel (NSTextView would otherwise start completion).
    func textView(_ v: NSTextView, doCommandBy sel: Selector) -> Bool {
        guard sel == #selector(NSResponder.cancelOperation(_:)) else { return false }
        view.window?.cancelOperation(nil)
        return true
    }

    func render() {
        queued = false
        let dark = view.effectiveAppearance.bestMatch(from: [.darkAqua, .aqua]) == .darkAqua
        p = pal(dark: dark)
        if m.phase != shownPhase {
            if case .done = m.phase { prompt.string = "" }      // a failed prompt stays, to retry
            if let (w, _) = statusWord(m.phase) {
                NSAccessibility.post(element: NSApp as Any, notification: .announcementRequested,
                                     userInfo: [.announcement: w.capitalized, .priority: NSAccessibilityPriorityLevel.high.rawValue])
            }
            shownPhase = m.phase
        }
        let focused = view.window?.firstResponder === prompt
        var items: [NSView] = [header(), promptSection()]
        if m.phase != .idle { items.append(replySection()) }
        items.append(footer())
        let c = vstack(items, 16, inset: edges(16, 16, 16, 16))
        c.translatesAutoresizingMaskIntoConstraints = false
        view.subviews.forEach { $0.removeFromSuperview() }
        view.addSubview(c)
        NSLayoutConstraint.activate([
            c.widthAnchor.constraint(equalToConstant: 360),
            c.topAnchor.constraint(equalTo: view.topAnchor),
            c.leadingAnchor.constraint(equalTo: view.leadingAnchor),
        ])
        col = c
        view.layer?.backgroundColor = p.paper.cgColor
        if focused { focusPrompt() }
        flushReply()
    }

    func header() -> NSView {
        let failed: Bool = if case .failed = m.phase { true } else { false }
        let meta = label(metaText(m.phase, now: Date()), 12, failed ? p.redInk : p.muted)
        meta.setContentCompressionResistancePriority(.init(100), for: .horizontal)
        self.meta = meta
        let top = hstack([hstack([clockMark(22, p), label("Clockify Agent", 15, .b, em: -0.02, p.ink)], 8), spacer(), meta], 12)
        return vstack([top, block(p.ink, h: 4)], 12)
    }

    func promptSection() -> NSView {
        let running: Bool = if case .running = m.phase { true } else { false }
        promptBox.layer?.borderColor = p.ink.cgColor
        promptBox.alphaValue = running ? 0.4 : 1
        prompt.isEditable = !running
        prompt.font = hvFont(15)
        prompt.textColor = p.ink
        prompt.insertionPointColor = p.ink
        // NSTextView draws this while empty; the key is long-standing but not public API, so check it first.
        if prompt.responds(to: NSSelectorFromString("setPlaceholderAttributedString:")) {
            prompt.setValue(NSAttributedString(string: "Log 2h to a Trello card, or ask “what did I log this week?”",
                                               attributes: [.font: hvFont(15), .foregroundColor: p.muted]), forKey: "placeholderAttributedString")
        }
        let action: Press
        if running {
            action = btn(.outline, p, "Stop", spin: true) { [unowned self] in m.onStop() }
        } else {
            action = btn(.red, p, "Send", enabled: m.repoError == nil) { [unowned self] in submit() }
            action.keyEquivalent = "\r"
            action.keyEquivalentModifierMask = .command
        }
        var items: [NSView] = [label("PROMPT", 11, .m, em: 0.06, p.muted), promptBox]
        if let e = m.repoError { items.append(label(e, 13, .b, p.redInk, wrap: 328)) }
        items.append(hstack([running ? spacer() : label("⌘↩ to send · Esc to close", 12, p.muted), spacer(), action], 10))
        return vstack(items, 8)
    }

    func submit() {
        let t = prompt.string.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !t.isEmpty, m.repoError == nil else { return }
        m.onSend(t)
    }

    func replySection() -> NSView {
        let (word, tone) = statusWord(m.phase) ?? ("", .muted)
        var status: [NSView] = []
        if case .running = m.phase { status.append(spinner(10, p.ink).0) } else if tone != .muted { status.append(block(p.text(tone), w: 8, h: 8)) }
        status.append(label(word, 11, .b, em: 0.04, p.text(tone)))
        let title = label("REPLY · " + promptTitle(m.asked), 11, .m, em: 0.06, p.muted)
        title.setContentCompressionResistancePriority(.init(100), for: .horizontal)
        let head = hstack([title, spacer(), hstack(status, 6)], 8)
        return vstack([vstack([head, block(p.ink, h: 1)], 6), replyScroll], 8)
    }

    /// Prose at 13 in ink, each Bash call as a muted "$ command" line; the result
    /// only when it adds something (it usually repeats the last prose); stderr's
    /// tail when a run failed without a result.
    func replyText() -> NSAttributedString {
        let s = NSMutableAttributedString()
        func add(_ t: String, _ size: CGFloat, _ c: NSColor, indent: CGFloat = 0) {
            let ps = NSMutableParagraphStyle()
            ps.paragraphSpacing = 6
            ps.firstLineHeadIndent = indent
            ps.headIndent = indent
            if s.length > 0 { s.append(NSAttributedString(string: "\n")) }
            s.append(NSAttributedString(string: t, attributes: [.font: hvFont(size), .foregroundColor: c, .paragraphStyle: ps]))
        }
        if m.trimmed { add("… earlier output trimmed", 12, p.muted) }
        var lastText: String?, hasResult = false
        for e in m.events {
            switch e {
            case .text(let t): add(t, 13, p.ink); lastText = t
            case .bash(let c): add("$ " + c, 12, p.muted, indent: 12)
            case .result(let t, let isError):
                hasResult = true
                if isError { add(t, 13, p.redInk) } else if t != lastText { add(t, 13, p.ink) }
            }
        }
        if case .failed = m.phase, !hasResult { m.stderrTail.forEach { add($0, 12, p.redInk) } }
        return s
    }

    func flushReply() {
        replyQueued = false
        guard col != nil else { return }
        reply.textStorage?.setAttributedString(replyText())
        fitReply()
        reply.scrollToEndOfDocument(nil)
    }

    /// Sizes the reply to its text, within the room the rest of the panel leaves under `m.cap`.
    func fitReply() {
        guard let col, let lm = reply.layoutManager, let tc = reply.textContainer else { return }
        col.layoutSubtreeIfNeeded()     // the reply needs its width before its text can be measured
        lm.ensureLayout(for: tc)
        let h = ceil(lm.usedRect(for: tc).height)
        let room = max(m.cap - (col.fittingSize.height - replyHeight.constant), 18)
        replyHeight.constant = min(max(h, 18), room)
        replyScroll.hasVerticalScroller = h > room
        m.onResize()
    }

    func footer() -> NSView {
        let row = hstack([
            txtBtn(p, "Open Log", .b, pad: (0, 8)) { [unowned self] in m.onLog() },
            spacer(),
            txtBtn(p, "Quit", .r, color: p.muted, pad: (8, 0), key: "q") { [unowned self] in m.onQuit() },
        ], 4)
        let c = label(claudeLine(m.claude), 12, p.muted), t = label(m.tray, 12, p.muted)
        claudeStat = c
        trayStat = t
        let chevron = fixed(NSImageView(image: NSImage(systemSymbolName: m.procsOpen ? "chevron.down" : "chevron.right", accessibilityDescription: nil)!
            .withSymbolConfiguration(.init(pointSize: 8, weight: .bold))!), w: 10)
        chevron.contentTintColor = p.muted
        let toggle = Press(hstack([chevron, c, spacer()], 4), fill: true) { [unowned self] in m.procsOpen.toggle() }
        toggle.paint = { [unowned self] _, hot in
            recolor(c, hot ? p.redInk : p.muted)
            chevron.contentTintColor = hot ? p.redInk : p.muted
        }
        toggle.setAccessibilityLabel("Claude processes")
        toggle.setAccessibilityValue(m.procsOpen ? "expanded" : "collapsed")
        var stats: [NSView] = [toggle]
        if m.procsOpen {
            let rows = vstack(procRowViews(), 2)
            procRows = rows
            stats.append(rows)
        } else {
            procRows = nil
        }
        stats.append(t)
        return vstack([block(p.ink, h: 1), row, vstack(stats, 3)], 3)
    }

    func procRowViews() -> [NSView] {
        guard let procs = m.claude, !procs.isEmpty else { return [] }
        func row(_ cells: [String], _ w: W, _ c: NSColor) -> NSView {
            let r = hstack(zip(cells, [44, 44, 56]).map { fixed(label($0, 12, w, c), w: CGFloat($1)) } + [stretch(label(cells[3], 12, w, c))], 6)
            r.edgeInsets.left = 14
            return r
        }
        return [row(["pid", "CPU", "Mem", "Process"], .b, p.ink)] + procs.map { row(procCells($0), .r, p.muted) }
    }

    /// Updates the stat lines, the meta line and an open process list in place.
    func updateStats() {
        guard view.window?.isVisible == true, let c = claudeStat, let t = trayStat else { return }
        c.attributedStringValue = label(claudeLine(m.claude), 12, p.muted).attributedStringValue
        t.attributedStringValue = label(m.tray, 12, p.muted).attributedStringValue
        if let meta, case .running = m.phase { meta.attributedStringValue = label(metaText(m.phase, now: Date()), 12, p.muted).attributedStringValue }
        if let rows = procRows {
            rows.arrangedSubviews.forEach { $0.removeFromSuperview() }
            procRowViews().forEach { rows.addArrangedSubview($0) }
            fitReply()      // more rows leave less room for the reply
        }
    }
}

private final class Root: NSView {
    var onAppearance: () -> Void = {}
    override var isFlipped: Bool { true }

    override func viewDidChangeEffectiveAppearance() {
        super.viewDidChangeEffectiveAppearance()
        onAppearance()
    }
}
