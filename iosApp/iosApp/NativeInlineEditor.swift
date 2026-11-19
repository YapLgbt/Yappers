import UIKit
import Yappers

extension NativeEditorFactory {
    func makeInlineEditor(controller: UIViewController, onHeightChange: @escaping (KotlinDouble) -> Void) -> AppleInlineEditor {
        NativeInlineEditor(controller: controller) { onHeightChange(KotlinDouble(value: Double($0))) }
    }
}

/// UIKit owns the selection, marked text and keyboard. Each accepted edit is written to the screen's Kotlin draft.
private final class NativeInlineEditor: UIView, AppleInlineEditor, UITextViewDelegate {
    var view: UIView { self }

    private weak var controller: UIViewController?
    private let onHeightChange: (CGFloat) -> Void
    private var model: InlineEditorModel?
    private var measuredHeight: CGFloat = 0
    private let textView = UITextView()
    private let placeholder = UILabel()
    private let send = UIButton(type: .system)
    private let dismissKeyboard = UIButton(type: .system)
    private let identity = UIButton(type: .system)
    private let claim = UIButton(type: .system)
    private let count = UILabel()
    private let rule = UIView()

    init(controller: UIViewController, onHeightChange: @escaping (CGFloat) -> Void) {
        self.controller = controller
        self.onHeightChange = onHeightChange
        super.init(frame: .zero)
        rule.backgroundColor = .separator
        textView.delegate = self
        textView.backgroundColor = .secondarySystemFill
        textView.textColor = .label
        textView.layer.cornerRadius = 14
        textView.layer.cornerCurve = .continuous
        textView.textContainerInset = UIEdgeInsets(top: 9, left: 12, bottom: 9, right: 12)
        textView.textContainer.lineFragmentPadding = 0
        textView.adjustsFontForContentSizeCategory = true
        placeholder.textColor = .secondaryLabel
        placeholder.isAccessibilityElement = false
        placeholder.isUserInteractionEnabled = false
        textView.addSubview(placeholder)

        send.setImage(UIImage(systemName: "arrow.up.circle.fill", withConfiguration: UIImage.SymbolConfiguration(textStyle: .title1)), for: .normal)
        send.addAction(UIAction { [weak self] _ in
            guard let self, let model = self.model else { return }
            let withinLimit = self.textView.text.utf16.count <= Int(model.limit)
            if self.textView.markedTextRange != nil { self.textView.unmarkText() }
            let committed = self.commitText()
            if withinLimit && committed { model.send() }
            self.updateVisibility()
        }, for: .touchUpInside)

        var configuration = UIButton.Configuration.plain()
        configuration.buttonSize = .small
        configuration.contentInsets = .zero
        configuration.titleLineBreakMode = .byTruncatingMiddle
        configuration.image = UIImage(systemName: "chevron.up.chevron.down", withConfiguration: UIImage.SymbolConfiguration(scale: .small))
        configuration.imagePlacement = .trailing
        configuration.imagePadding = 4
        identity.configuration = configuration
        identity.contentHorizontalAlignment = .leading
        identity.showsMenuAsPrimaryAction = true

        claim.setTitle("Choose a username", for: .normal)
        claim.contentHorizontalAlignment = .leading
        claim.titleLabel?.adjustsFontForContentSizeCategory = true
        claim.addAction(UIAction { [weak self] _ in
            guard let self, let controller = self.controller, let claim = self.model?.usernameClaim else { return }
            NativeUsernameClaim.present(from: controller, claim: claim)
        }, for: .touchUpInside)

        count.font = .preferredFont(forTextStyle: .caption1)
        count.adjustsFontForContentSizeCategory = true
        count.textColor = .secondaryLabel
        count.textAlignment = .right
        dismissKeyboard.setImage(UIImage(systemName: "keyboard.chevron.compact.down"), for: .normal)
        dismissKeyboard.accessibilityLabel = "Dismiss keyboard"
        dismissKeyboard.addAction(UIAction { [weak self] _ in self?.textView.resignFirstResponder() }, for: .touchUpInside)
        [rule, textView, send, dismissKeyboard, identity, claim, count].forEach(addSubview)
        keyboardLayoutGuide.usesBottomSafeArea = false
        NotificationCenter.default.addObserver(self, selector: #selector(keyboardFrameChanged),
                                               name: UIResponder.keyboardDidChangeFrameNotification, object: nil)
        registerForTraitChanges([UITraitPreferredContentSizeCategory.self]) { (editor: NativeInlineEditor, _: UITraitCollection) in
            editor.updateFonts()
            editor.setNeedsLayout()
        }
    }

    required init?(coder: NSCoder) { fatalError("Use init(controller:onHeightChange:)") }

    @objc private func keyboardFrameChanged() { setNeedsLayout() }

    private func updateFonts() {
        textView.font = model?.namedIdentity == nil ? .preferredFont(forTextStyle: .body) : NativeWritingType.publicBody
        placeholder.font = textView.font
        claim.titleLabel?.font = .preferredFont(forTextStyle: .callout)
    }

    func render(model next: InlineEditorModel) {
        let previous = model
        model = next
        let reply = next.namedIdentity != nil
        if previous == nil || (previous?.namedIdentity == nil) != (next.namedIdentity == nil) {
            updateFonts()
            textView.accessibilityLabel = reply ? "Write a reply" : "Message"
            textView.accessibilityIdentifier = reply ? "reply-body" : "direct-message-body"
            placeholder.text = textView.accessibilityLabel
            send.accessibilityLabel = reply ? "Post comment" : "Send"
        }
        // Leave UIKit's insertion point, marked text, and undo history alone on ordinary renders.
        let body = next.body
        if textView.markedTextRange == nil && textView.text != body { textView.text = body }
        if let previous, previous.dismissKeyboardRequest != next.dismissKeyboardRequest { textView.resignFirstResponder() }
        identity.isEnabled = !next.sending
        claim.isEnabled = !next.sending
        if let name = next.namedIdentity, previous?.namedIdentity != name || previous?.anonymous != next.anonymous || previous?.networked != next.networked {
            let title = "Reply as " + (next.anonymous ? "Anonymous" : name)
            identity.configuration?.title = title
            identity.accessibilityLabel = title
            let options = [(true, "Anonymous"), (false, name)].map { anonymous, title in
                UIAction(title: title, state: next.anonymous == anonymous ? .on : .off) { [weak self] _ in
                    self?.model?.chooseIdentity(anonymous: anonymous)
                }
            }
            identity.menu = UIMenu(title: next.networked ? "Your home can identify anonymous posts." : "Posting identity", children: options)
        }
        updateVisibility()
    }

    func textView(_ textView: UITextView, shouldChangeTextIn range: NSRange, replacementText text: String) -> Bool {
        guard let model, !model.sending else { return false }
        return textView.text.utf16.count - range.length + text.utf16.count <= Int(model.limit)
    }

    // Dictation, autocorrection and input methods can bypass the proposed-change delegate.
    @discardableResult
    private func commitText() -> Bool {
        guard let model else { return false }
        let text = textView.text ?? ""
        if text == model.body { return true }
        if model.edit(body: text) { return true }
        if textView.markedTextRange == nil {
            let selection = textView.selectedRange
            let accepted = model.body
            textView.text = accepted
            textView.selectedRange = NSRange(location: min(selection.location, accepted.utf16.count), length: 0)
        }
        return false
    }

    func textViewDidChange(_ textView: UITextView) {
        commitText()
        updateVisibility()
    }

    func textViewDidBeginEditing(_ textView: UITextView) { updateVisibility() }
    func textViewDidEndEditing(_ textView: UITextView) {
        commitText()
        updateVisibility()
    }

    private func updateVisibility() {
        guard let model else { return }
        send.isEnabled = model.canSend && textView.text.utf16.count <= Int(model.limit)
        let editing = textView.isFirstResponder || !textView.text.isEmpty || model.sending
        placeholder.isHidden = !textView.text.isEmpty
        send.isHidden = !editing
        identity.isHidden = model.namedIdentity == nil || !editing
        dismissKeyboard.isHidden = !textView.isFirstResponder
        count.isHidden = !editing || (model.namedIdentity == nil && textView.text.utf16.count < Int(model.limit) - 100)
        claim.isHidden = !editing || model.usernameClaim == nil
        count.text = "\(textView.text.utf16.count) / \(model.limit)"
        count.accessibilityLabel = "\(textView.text.utf16.count) of \(model.limit) characters"
        setNeedsLayout()
    }

    override func layoutSubviews() {
        super.layoutSubviews()
        guard bounds.width > 0, model != nil else { return }
        let inset: CGFloat = model?.namedIdentity == nil ? 0 : 16
        let width = max(1, bounds.width - 2 * inset)
        let sendWidth: CGFloat = send.isHidden ? 0 : 48
        let dismissWidth: CGFloat = dismissKeyboard.isHidden ? 0 : 44
        let fieldWidth = max(1, width - sendWidth - (identity.isHidden ? dismissWidth : 0))
        let line = textView.font?.lineHeight ?? 20
        let limit = max(44, min(ceil(4 * line) + 18, (window?.bounds.height ?? 800) * 0.21))
        let natural = ceil(textView.sizeThatFits(CGSize(width: fieldWidth, height: .greatestFiniteMagnitude)).height)
        let fieldHeight = max(44, min(limit, natural))
        var y: CGFloat = 6
        rule.frame = CGRect(x: 0, y: 0, width: bounds.width, height: 1 / traitCollection.displayScale)
        textView.frame = CGRect(x: inset, y: y, width: fieldWidth, height: fieldHeight)
        let scrolls = natural > fieldHeight
        if textView.isScrollEnabled != scrolls { textView.isScrollEnabled = scrolls }
        placeholder.frame = CGRect(x: 12, y: 9, width: max(1, fieldWidth - 24), height: ceil(line))
        send.frame = CGRect(x: bounds.width - inset - sendWidth, y: y + fieldHeight - 44, width: sendWidth, height: 44)
        if identity.isHidden {
            dismissKeyboard.frame = CGRect(x: inset + fieldWidth, y: y + fieldHeight - 44, width: dismissWidth, height: 44)
        }
        y += fieldHeight

        let countSize = count.isHidden ? .zero : count.intrinsicContentSize
        if !identity.isHidden {
            let rowHeight = max(44, ceil(identity.intrinsicContentSize.height))
            let separateCount = traitCollection.preferredContentSizeCategory.isAccessibilityCategory
            dismissKeyboard.frame = CGRect(x: inset + width - dismissWidth, y: y, width: dismissWidth, height: rowHeight)
            let reserved = dismissWidth + (separateCount || count.isHidden ? 0 : countSize.width + 12)
            identity.frame = CGRect(x: inset, y: y, width: max(1, width - reserved), height: rowHeight)
            if !separateCount {
                count.frame = CGRect(x: inset + width - dismissWidth - countSize.width, y: y, width: countSize.width, height: rowHeight)
            }
            y += rowHeight
            if separateCount && !count.isHidden {
                count.frame = CGRect(x: inset, y: y, width: width, height: ceil(countSize.height))
                y += ceil(countSize.height)
            }
        } else if !count.isHidden {
            count.frame = CGRect(x: inset, y: y, width: width, height: ceil(countSize.height))
            y += ceil(countSize.height)
        }
        if !claim.isHidden {
            let height = max(44, claim.intrinsicContentSize.height)
            claim.frame = CGRect(x: inset, y: y, width: width, height: height)
            y += height
        }
        // Compose accounts for its own IME and safe areas. UIKit reports any remaining
        // intersection in this view's coordinates, including the keyboard's suggestion bar.
        let overlap = textView.isFirstResponder && window != nil ? bounds.intersection(keyboardLayoutGuide.layoutFrame).height : 0
        let height = ceil(y + 6 + overlap)
        if height != measuredHeight {
            measuredHeight = height
            onHeightChange(height)
        }
    }
}
