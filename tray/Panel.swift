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

/// One open Trello card from the dashboard's /api/search.
struct CardHit: Decodable, Equatable {
    let number: Int
    let name: String
    let url: String
    let board: String
}

/// What the Find card section shows under its field.
enum CardResults: Equatable {
    case empty                  // under 2 characters typed
    case searching              // the first search for this text is still out
    case found([CardHit])
    case failed(String)
}

/// The picked row after ↑ (-1) or ↓ (+1), wrapping at either end.
func nextSel(_ sel: Int, _ delta: Int, count: Int) -> Int {
    count == 0 ? 0 : ((sel + delta) % count + count) % count
}

/// The panel's colours: follow macOS, or always light or dark. Kept in the defaults as its raw value.
enum Theme: String, CaseIterable {
    case auto, light, dark
    /// What NSApp.appearance gets; nil follows macOS.
    var appearance: NSAppearance? { self == .auto ? nil : NSAppearance(named: self == .dark ? .darkAqua : .aqua) }
}

/// The panel's column, and the search field and its dropdown inside its 16 pt margins.
let panelWidth: CGFloat = 420, dropWidth = panelWidth - 32

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
    var tailnet: URL?                                    // the dashboard over Tailscale; nil hides its button
    var cap: CGFloat = 400                               // tallest the whole panel may be; Tray sets it from the screen
    // The card search updates its rows in place (cardsChanged): a rebuild would end the
    // field's editing, an input method's composition with it, and jump the reply to its end.
    var results = CardResults.empty { didSet { cardSel = 0; cardsChanged() } }
    var cardSel = 0 { didSet { cardsChanged() } }        // the row ↩ copies
    var copied: String? { didSet { cardsChanged() } }    // the URL just copied; its row says COPIED
    var stale = false { didSet { cardsChanged() } }      // a newer search is out: rows dim and ↩ waits
    var pendingCopy = false                              // ↩ came while stale: copy the newest answer's top card
    var dropOpen = false { didSet { cardsChanged() } }   // typing opens the dropdown; Esc, leaving the field or a copy closes it
    var theme = Theme.auto { didSet { changed() } }
    // Set by PanelController.
    var changed: () -> Void = {}
    var replyChanged: () -> Void = {}
    var statsChanged: () -> Void = {}
    var cardsChanged: () -> Void = {}
    // Set by Tray.
    var onSend: (String) -> Void = { _ in }
    var onStop: () -> Void = {}
    var onLog: () -> Void = {}
    var onOpen: (URL) -> Void = { _ in }
    var onSearch: (String) -> Void = { _ in }
    var onCopy: (CardHit) -> Void = { _ in }
    var onTheme: (Theme) -> Void = { _ in }
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

/// Claude's markdown, inline only: bold, italic, `code`, strikethrough and links, newlines kept.
/// AppKit draws none of the parsed intents, so each maps to a font here; text that fails to parse stays plain.
/// shortcut: headings, lists and tables show as their markdown source; map presentationIntent if they need styling.
func markdown(_ t: String, _ size: CGFloat, _ c: NSColor) -> NSAttributedString {
    guard let md = try? AttributedString(markdown: t, options: .init(interpretedSyntax: .inlineOnlyPreservingWhitespace)) else {
        return NSAttributedString(string: t, attributes: [.font: hvFont(size), .foregroundColor: c])
    }
    let s = NSMutableAttributedString()
    for r in md.runs {
        let i = r.inlinePresentationIntent ?? []
        var f = i.contains(.code) ? NSFont.monospacedSystemFont(ofSize: size - 1, weight: .regular) : hvFont(size, i.contains(.stronglyEmphasized) ? .b : .r)
        if i.contains(.emphasized) { f = NSFontManager.shared.convert(f, toHaveTrait: .italicFontMask) }
        var a: [NSAttributedString.Key: Any] = [.font: f, .foregroundColor: c]
        if i.contains(.strikethrough) { a[.strikethroughStyle] = NSUnderlineStyle.single.rawValue }
        // The text can come from Trello cards, so the tooltip shows where a link really goes.
        if let l = r.link { a[.link] = l; a[.toolTip] = l.absoluteString }
        s.append(NSAttributedString(string: String(md[r.range].characters), attributes: a))
    }
    return s
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

/// Has VoiceOver say `s` now.
func announce(_ s: String) {
    NSAccessibility.post(element: NSApp as Any, notification: .announcementRequested,
                         userInfo: [.announcement: s, .priority: NSAccessibilityPriorityLevel.high.rawValue])
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
    override func acceptsFirstMouse(for e: NSEvent?) -> Bool { true }   // the card dropdown's window never becomes key

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

// MARK: - Panel

/// The panel's content. A state change rebuilds it; reply text and stats update in
/// place. The prompt, reply and search fields are made once and moved into each rebuild,
/// so typed text survives.
final class PanelController: NSViewController, NSTextViewDelegate, NSTextFieldDelegate {
    let m: PanelModel
    private var queued = false, replyQueued = false, cardsQueued = false
    private let promptScroll = NSTextView.scrollableTextView(), replyScroll = NSTextView.scrollableTextView()
    private var prompt: NSTextView { promptScroll.documentView as! NSTextView }
    private var reply: NSTextView { replyScroll.documentView as! NSTextView }
    private let promptBox = NSView()      // the prompt's 2 pt border: NSScrollView keeps no border on its own layer
    private let search = NSTextField(), searchBox = NSView()
    private lazy var replyHeight = replyScroll.heightAnchor.constraint(equalToConstant: 18)
    private var col: NSView?
    private var meta: NSTextField?, claudeStat: NSTextField?, trayStat: NSTextField?, procRows: Stack?, statToggle: Press?
    private let cardArea = vstack([], 0)
    /// The card dropdown: a child window, so it can hang over the footer and past the panel's
    /// bottom. Borderless, so it never becomes key and the search field keeps the keyboard.
    lazy var drop: NSPanel = {
        let w = NSPanel(contentRect: NSRect(x: 0, y: 0, width: dropWidth, height: 40), styleMask: [.borderless, .nonactivatingPanel], backing: .buffered, defer: true)
        w.level = .popUpMenu
        w.hasShadow = true
        w.hidesOnDeactivate = false
        w.isReleasedWhenClosed = false
        let c = NSView()
        c.wantsLayer = true
        c.layer?.borderWidth = 2
        cardArea.translatesAutoresizingMaskIntoConstraints = false
        c.addSubview(cardArea)
        NSLayoutConstraint.activate([
            cardArea.topAnchor.constraint(equalTo: c.topAnchor, constant: 2),
            cardArea.leadingAnchor.constraint(equalTo: c.leadingAnchor, constant: 2),
            cardArea.widthAnchor.constraint(equalToConstant: dropWidth - 4),
        ])
        w.contentView = c
        return w
    }()
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
        m.cardsChanged = { [unowned self] in
            guard !cardsQueued else { return }
            cardsQueued = true
            DispatchQueue.main.async { [unowned self] in
                cardsQueued = false
                updateCards()
            }
        }
    }

    required init?(coder: NSCoder) { fatalError() }

    override func loadView() {
        let r = Root()
        r.wantsLayer = true
        r.onAppearance = { [unowned self] in m.changed() }     // batched, and skipped while hidden: the next open renders
        r.onFind = { [unowned self] in view.window?.makeFirstResponder(search) }
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
        search.isBordered = false
        search.drawsBackground = false
        search.focusRingType = .none
        search.usesSingleLineMode = true
        search.cell?.isScrollable = true
        search.delegate = self
        search.setAccessibilityLabel("Search Trello cards")
        search.translatesAutoresizingMaskIntoConstraints = false
        searchBox.wantsLayer = true
        searchBox.layer?.borderWidth = 2
        searchBox.translatesAutoresizingMaskIntoConstraints = false
        searchBox.addSubview(search)
        NSLayoutConstraint.activate([
            searchBox.heightAnchor.constraint(equalToConstant: 36),
            search.centerYAnchor.constraint(equalTo: searchBox.centerYAnchor),
            search.leadingAnchor.constraint(equalTo: searchBox.leadingAnchor, constant: 8),
            search.trailingAnchor.constraint(equalTo: searchBox.trailingAnchor, constant: -8),
        ])
    }

    func focusPrompt() { view.window?.makeFirstResponder(prompt) }

    func fittingSize() -> NSSize { col?.fittingSize ?? .zero }

    /// Esc in the prompt closes the panel (NSTextView would otherwise start completion).
    func textView(_ v: NSTextView, doCommandBy sel: Selector) -> Bool {
        guard sel == #selector(NSResponder.cancelOperation(_:)) else { return false }
        view.window?.cancelOperation(nil)
        return true
    }

    func controlTextDidChange(_ n: Notification) {
        m.dropOpen = true
        m.onSearch(search.stringValue)
    }

    func controlTextDidEndEditing(_ n: Notification) { m.dropOpen = false }

    /// In the search field: ↑↓ open the dropdown or pick a card, ↩ copies it, Esc closes the dropdown, then the panel.
    func control(_ c: NSControl, textView: NSTextView, doCommandBy sel: Selector) -> Bool {
        let cards: [CardHit] = if case .found(let f) = m.results { f } else { [] }
        func pick(_ d: Int) {
            guard m.dropOpen else { m.dropOpen = true; return }
            m.cardSel = nextSel(m.cardSel, d, count: cards.count)
            if cards.indices.contains(m.cardSel) { announce("#\(cards[m.cardSel].number) \(cards[m.cardSel].name)") }
        }
        switch sel {
        case #selector(NSResponder.moveDown(_:)): pick(1)
        case #selector(NSResponder.moveUp(_:)): pick(-1)
        case #selector(NSResponder.insertNewline(_:)):
            if m.stale { m.pendingCopy = true } else if m.dropOpen, cards.indices.contains(m.cardSel) { m.onCopy(cards[m.cardSel]) }
        case #selector(NSResponder.cancelOperation(_:)): if m.dropOpen, m.results != .empty { m.dropOpen = false } else { view.window?.cancelOperation(nil) }
        default: return false
        }
        return true
    }

    func render() {
        queued = false
        let dark = view.effectiveAppearance.bestMatch(from: [.darkAqua, .aqua]) == .darkAqua
        p = pal(dark: dark)
        if m.phase != shownPhase {
            if case .done = m.phase { prompt.string = "" }      // a failed prompt stays, to retry
            if let (w, _) = statusWord(m.phase) { announce(w.capitalized) }
            shownPhase = m.phase
        }
        let focused = view.window?.firstResponder === prompt
        let caret = search.currentEditor()?.selectedRange     // the rebuild ends the search's editing; this restores it
        let open = m.dropOpen                                  // and ending it closes the dropdown
        var items: [NSView] = [header(), promptSection()]
        if m.phase != .idle { items.append(replySection()) }
        items.append(cardSection())
        items.append(footer())
        let c = vstack([vstack(items, 16, inset: edges(16, 16, 6, 16)), statusBar()], 0)
        c.translatesAutoresizingMaskIntoConstraints = false
        view.subviews.forEach { $0.removeFromSuperview() }
        view.addSubview(c)
        NSLayoutConstraint.activate([
            c.widthAnchor.constraint(equalToConstant: panelWidth),
            c.topAnchor.constraint(equalTo: view.topAnchor),
            c.leadingAnchor.constraint(equalTo: view.leadingAnchor),
        ])
        col = c
        view.layer?.backgroundColor = p.paper.cgColor
        if focused { focusPrompt() }
        if let caret, view.window?.makeFirstResponder(search) == true {
            search.currentEditor()?.selectedRange = caret
            m.dropOpen = open
        }
        flushReply()
        m.cardsChanged()        // the dropdown's colours; after the window shows, on the first open
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
        if let e = m.repoError { items.append(label(e, 13, .b, p.redInk, wrap: panelWidth - 32)) }
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

    /// Prose at 13 in ink, rendered from markdown; each Bash call as a muted "$ command" line; the result
    /// only when it adds something (it usually repeats the last prose); stderr's
    /// tail when a run failed without a result.
    func replyText() -> NSAttributedString {
        let s = NSMutableAttributedString()
        func add(_ t: String, _ size: CGFloat, _ c: NSColor, indent: CGFloat = 0, md: Bool = false) {
            let ps = NSMutableParagraphStyle()
            ps.paragraphSpacing = 6
            ps.firstLineHeadIndent = indent
            ps.headIndent = indent
            if s.length > 0 { s.append(NSAttributedString(string: "\n")) }
            let a = NSMutableAttributedString(attributedString: md ? markdown(t, size, c)
                : NSAttributedString(string: t, attributes: [.font: hvFont(size), .foregroundColor: c]))
            a.addAttribute(.paragraphStyle, value: ps, range: NSRange(location: 0, length: a.length))
            s.append(a)
        }
        if m.trimmed { add("… earlier output trimmed", 12, p.muted) }
        var lastText: String?, hasResult = false
        for e in m.events {
            switch e {
            case .text(let t): add(t, 13, p.ink, md: true); lastText = t
            case .bash(let c): add("$ " + c, 12, p.muted, indent: 12)
            case .result(let t, let isError):
                hasResult = true
                if isError { add(t, 13, p.redInk) } else if t != lastText { add(t, 13, p.ink, md: true) }
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
        if drop.parent != nil { placeDrop() }
    }

    /// FIND CARD: a one-line search; its up to 10 open cards drop down over the footer. A click or ↩ copies a card's URL.
    func cardSection() -> NSView {
        searchBox.layer?.borderColor = p.ink.cgColor
        search.font = hvFont(15)
        search.textColor = p.ink
        search.placeholderAttributedString = NSAttributedString(string: "Search Trello cards", attributes: [.font: hvFont(15), .foregroundColor: p.muted])
        return vstack([label("FIND CARD", 11, .m, em: 0.06, p.muted), searchBox], 8)
    }

    /// The dropdown's rows, or one line saying why there are none.
    func fillCards() {
        func note(_ l: NSTextField) -> NSView { vstack([l], 0, inset: edges(7, 8, 7, 8)) }
        let views: [NSView] = switch m.results {
        case .empty: []
        case .searching: [note(label("Searching…", 12, p.muted))]
        case .failed(let why): [note(label(why, 13, .b, p.redInk, wrap: dropWidth - 20))]
        case .found(let cards) where cards.isEmpty: [note(label("No open cards", 12, p.muted))]
        case .found(let cards):
            cards.enumerated().flatMap { i, c in
                (i > 0 ? [block(p.muted.withAlphaComponent(0.3), h: 1)] : []) + [cardRow(c, i)]
            }
        }
        for v in views {
            cardArea.addArrangedSubview(v)
            v.widthAnchor.constraint(equalTo: cardArea.widthAnchor).isActive = true
        }
        cardArea.alphaValue = m.stale ? 0.4 : 1
    }

    /// Refills the dropdown in place (a panel rebuild would end the search's editing), then shows or hides it.
    func updateCards() {
        guard view.window?.isVisible == true else { return }
        cardArea.arrangedSubviews.forEach { $0.removeFromSuperview() }
        fillCards()
        placeDrop()
    }

    /// Hangs the dropdown from the search field, its top border on the field's bottom one.
    func placeDrop() {
        guard m.dropOpen, m.results != .empty, let win = view.window, win.isVisible else { return hideDrop() }
        drop.contentView?.layer?.borderColor = p.ink.cgColor
        drop.contentView?.layer?.backgroundColor = p.paper.cgColor
        view.layoutSubtreeIfNeeded()
        cardArea.layoutSubtreeIfNeeded()
        let box = win.convertToScreen(searchBox.convert(searchBox.bounds, to: nil))
        let floor = (win.screen ?? NSScreen.main)?.visibleFrame.minY ?? 0
        // shortcut: rows below the screen's bottom edge are cut off; add a scroll view if that bites.
        let h = min(cardArea.fittingSize.height + 4, box.minY + 2 - floor)
        drop.setFrame(NSRect(x: box.minX, y: box.minY + 2 - h, width: box.width, height: h), display: true)
        if drop.parent == nil { win.addChildWindow(drop, ordered: .above) }
    }

    func hideDrop() {
        drop.parent?.removeChildWindow(drop)
        drop.orderOut(nil)
    }

    /// "#482  Login fails  RMS"; ink fill while hot or picked, like a button. COPIED replaces the board for 1.5 s.
    func cardRow(_ c: CardHit, _ i: Int) -> NSView {
        let sel = i == m.cardSel
        let copied = m.copied == c.url
        let num = label("#\(c.number)", 12, .m, p.muted), name = label(c.name, 13, .b, p.ink)
        let side = copied ? label("COPIED", 11, .b, em: 0.04, p.redInk) : label(c.board, 12, p.muted)
        name.setContentCompressionResistancePriority(.init(100), for: .horizontal)
        let r = Press(hstack([fixed(num, w: 44), stretch(name), side], 8), pad: (8, 8), minHeight: 30, fill: true) { [unowned self] in
            m.cardSel = i
            m.onCopy(c)
        }
        r.paint = { [unowned self] r, hot in
            let on = hot || sel
            r.layer?.backgroundColor = (on ? p.ink : .clear).cgColor
            recolor(num, on ? p.paper : p.muted)
            recolor(name, on ? p.paper : p.ink)
            recolor(side, on ? p.paper : copied ? p.redInk : p.muted)
        }
        r.toolTip = c.url
        r.setAccessibilityLabel("Copy link to #\(c.number) \(c.name), \(c.board)")
        return r
    }

    /// The links, and the process list when it's open; the status bar goes under them, flush with the edges.
    func footer() -> NSView {
        let links: [NSView?] = [
            link("Open Log", pad: (0, 8)) { [unowned self] in m.onLog() },
            link("Dashboard", ext: true, pad: (8, 8)) { [unowned self] in m.onOpen(dashboardURL) },
            m.tailnet.map { url in link("Via Tailscale", ext: true, pad: (8, 8)) { [unowned self] in m.onOpen(url) } },
        ]
        var items: [NSView] = [block(p.ink, h: 1), hstack(links.compactMap { $0 } + [spacer()], 4)]
        procRows = nil
        if m.procsOpen {
            let rows = vstack(procRowViews(), 2)
            procRows = rows
            items.append(rows)
        }
        return vstack(items, 3)
    }

    /// A footer link, red ink while hot; ↗ marks the ones that open a browser.
    func link(_ title: String, ext: Bool = false, pad: (CGFloat, CGFloat), _ act: @escaping () -> Void) -> Press {
        let l = label(title, 13, .b, p.ink), arrow = label("↗", 13, p.muted)
        let b = Press(hstack(ext ? [l, arrow] : [l], 2), pad: pad, minHeight: 36, act)
        b.paint = { [unowned self] _, hot in
            recolor(l, hot ? p.redInk : p.ink)
            recolor(arrow, hot ? p.redInk : p.muted)
        }
        b.setAccessibilityLabel(title)
        return b
    }

    /// Small capitals on the status bar.
    func barText(_ s: String, _ c: NSColor, _ w: W = .b) -> NSTextField { label(s.uppercased(), 10.5, w, em: 0.04, c) }

    /// The ink bar along the bottom edge, like the header's rule: Claude's state (a red square while
    /// it runs; a click opens the process list), the tray's usage, the theme and Quit.
    func statusBar() -> NSView {
        let fg = p.paper, dim = p.paper.withAlphaComponent(0.6)
        let running: Bool = if case .running = m.phase { true } else { false }
        let c = barText(claudeLine(m.claude), fg), t = barText(m.tray, dim, .m)
        claudeStat = c
        trayStat = t
        c.setContentCompressionResistancePriority(.init(200), for: .horizontal)
        t.setContentCompressionResistancePriority(.init(100), for: .horizontal)
        let toggle = stretch(Press(hstack([block(running ? swissRed : fg, w: 7, h: 7), c, t, spacer()], 8), minHeight: 34, fill: true) { [unowned self] in
            m.procsOpen.toggle()
        })
        toggle.paint = { _, hot in recolor(c, hot ? swissRed : fg) }
        toggle.setAccessibilityLabel("Claude processes")
        statToggle = toggle
        statsSpoken()
        let themes = Theme.allCases.map { th in
            let on = th == m.theme
            let l = barText(th.rawValue, on ? fg : dim)
            let b = Press(vstack([l, block(on ? swissRed : .clear, h: 2)], 2), minHeight: 34) { [unowned self] in m.onTheme(th) }
            b.paint = { _, hot in recolor(l, hot ? swissRed : on ? fg : dim) }
            b.setAccessibilityLabel("\(th.rawValue.capitalized) theme" + (on ? ", selected" : ""))
            return b
        }
        let q = barText("Quit", dim)
        let quit = Press(q, pad: (10, 0), minHeight: 34) { [unowned self] in m.onQuit() }
        quit.paint = { _, hot in recolor(q, hot ? swissRed : dim) }
        quit.keyEquivalent = "q"
        quit.keyEquivalentModifierMask = .command
        quit.setAccessibilityLabel("Quit")
        let row = hstack([toggle, hstack(themes, 8), block(dim, w: 1, h: 14), quit], 10)
        let bar = vstack([row], 0, inset: edges(0, 16, 0, 16))
        bar.wantsLayer = true
        bar.layer?.backgroundColor = p.ink.cgColor
        return bar
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

    /// The toggle hides its labels from VoiceOver, so its value reads them.
    func statsSpoken() {
        statToggle?.setAccessibilityValue("\(m.procsOpen ? "expanded" : "collapsed"), \(claudeLine(m.claude)), \(m.tray)")
    }

    /// Updates the stat lines, the meta line and an open process list in place.
    func updateStats() {
        guard view.window?.isVisible == true, let c = claudeStat, let t = trayStat else { return }
        c.attributedStringValue = barText(claudeLine(m.claude), p.paper).attributedStringValue
        t.attributedStringValue = barText(m.tray, p.paper.withAlphaComponent(0.6), .m).attributedStringValue
        statsSpoken()
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
    var onFind: () -> Void = {}
    override var isFlipped: Bool { true }

    /// ⌘F goes to the card search.
    override func performKeyEquivalent(with e: NSEvent) -> Bool {
        guard e.modifierFlags.intersection([.command, .shift, .option, .control]) == .command, e.charactersIgnoringModifiers?.lowercased() == "f" else {
            return super.performKeyEquivalent(with: e)
        }
        onFind()
        return true
    }

    override func viewDidChangeEffectiveAppearance() {
        super.viewDidChangeEffectiveAppearance()
        onAppearance()
    }
}
