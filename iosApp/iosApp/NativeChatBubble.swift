import UIKit
import Yappers

extension NativeEditorFactory {
    func makeChatBubble() -> AppleChatBubbleControl { NativeChatBubble() }
}

private final class NativeChatBubble: UIView, AppleChatBubbleControl, UIContextMenuInteractionDelegate {
    var view: UIView { self }
    private let label = UILabel()
    private var bubble: ChatBubble?
    private let tailWidth: CGFloat = 6
    private var tailOnRight: Bool {
        (bubble?.outgoing == true) != (effectiveUserInterfaceLayoutDirection == .rightToLeft)
    }

    init() {
        super.init(frame: .zero)
        contentMode = .redraw
        label.numberOfLines = 0
        label.font = .preferredFont(forTextStyle: .body)
        label.adjustsFontForContentSizeCategory = true
        label.isAccessibilityElement = false
        addSubview(label)
        addInteraction(UIContextMenuInteraction(delegate: self))
        isAccessibilityElement = true
        accessibilityTraits = .staticText
    }

    @available(*, unavailable)
    required init?(coder: NSCoder) { fatalError() }

    func render(bubble: ChatBubble) {
        self.bubble = bubble
        label.text = bubble.text
        label.textColor = bubble.outgoing ? .systemBackground : .label
        accessibilityLabel = bubble.text
        accessibilityValue = bubble.sender
        var actions = [UIAccessibilityCustomAction(name: "Copy") { [weak self] _ in self?.copyMessage(); return true }]
        if bubble.canReport {
            actions.append(UIAccessibilityCustomAction(name: "Report message") { _ in bubble.report(); return true })
        }
        accessibilityCustomActions = actions
        setNeedsDisplay()
        setNeedsLayout()
    }

    override func sizeThatFits(_ size: CGSize) -> CGSize {
        let horizontal = 24 + tailWidth
        let fit = label.sizeThatFits(CGSize(width: max(1, size.width - horizontal), height: .greatestFiniteMagnitude))
        return CGSize(width: min(size.width, max(38, ceil(fit.width) + horizontal)), height: ceil(fit.height) + 14)
    }

    override func layoutSubviews() {
        super.layoutSubviews()
        let body = bounds.inset(by: UIEdgeInsets(top: 7, left: 12, bottom: 7, right: 12))
        label.frame = body.inset(by: UIEdgeInsets(top: 0, left: tailOnRight ? 0 : tailWidth,
                                                 bottom: 0, right: tailOnRight ? tailWidth : 0))
    }

    private func outline() -> UIBezierPath {
        let w = bounds.width, h = bounds.height, x = tailWidth
        let r = min(18, h / 2, max(0, (w - x) / 2))
        let path = UIBezierPath(roundedRect: CGRect(x: x, y: 0, width: max(0, w - x), height: h), cornerRadius: r)
        if bubble?.endsGroup == true {
            let tail = UIBezierPath()
            tail.move(to: CGPoint(x: x, y: h - 18))
            tail.addQuadCurve(to: CGPoint(x: 0, y: h), controlPoint: CGPoint(x: x + 1, y: h - 5))
            tail.addQuadCurve(to: CGPoint(x: x + 14, y: h - 4), controlPoint: CGPoint(x: x + 5, y: h))
            tail.addLine(to: CGPoint(x: x + 13, y: h - 17))
            tail.close()
            path.append(tail.reversing())
        }
        if tailOnRight { path.apply(CGAffineTransform(a: -1, b: 0, c: 0, d: 1, tx: w, ty: 0)) }
        return path
    }

    override func draw(_ rect: CGRect) {
        let color = bubble?.outgoing == true ? YapTabBarController.actionTint : .secondarySystemBackground
        color.setFill()
        outline().fill()
    }

    private func copyMessage() { UIPasteboard.general.string = bubble?.text }

    func contextMenuInteraction(_ interaction: UIContextMenuInteraction,
                                configurationForMenuAtLocation location: CGPoint) -> UIContextMenuConfiguration? {
        guard let bubble else { return nil }
        return UIContextMenuConfiguration(identifier: nil, previewProvider: nil) { _ in
            var actions = [UIAction(title: "Copy", image: UIImage(systemName: "doc.on.doc")) { _ in
                UIPasteboard.general.string = bubble.text
            }]
            if bubble.canReport {
                actions.append(UIAction(title: "Report message", image: UIImage(systemName: "flag"), attributes: .destructive) { _ in
                    bubble.report()
                })
            }
            return UIMenu(children: actions)
        }
    }
}
