import UIKit
import ImageIO
import Yappers

final class NativeFormCell: UITableViewCell, UITextFieldDelegate, UITextViewDelegate, UISearchBarDelegate {
    private let kind: String
    private let standaloneField: Bool
    private var avatar: UIImage?
    private var row: FormRow?
    private var hasMarkedEdit = false
    private lazy var field = UITextField()
    private lazy var editor = UITextView()
    private lazy var search = UISearchBar()
    private lazy var toggle = UISwitch()
    private lazy var options = UIButton(type: .system)
    private lazy var segments = UISegmentedControl()
    private lazy var label = UILabel()
    private lazy var hint = UILabel()
    private lazy var imagePreview = UIImageView()
    private var stacked: Bool { ["field", "secure", "multiline", "selectable", "search", "segments", "photo", "menu", "text"].contains(kind) }

    init(kind: String, standaloneField: Bool = false) {
        self.kind = kind
        self.standaloneField = standaloneField
        super.init(style: .default, reuseIdentifier: kind)
        if stacked { configureStack() }
        if kind == "toggle" {
            toggle.addTarget(self, action: #selector(switched), for: .valueChanged)
            accessoryView = toggle
        }
    }
    required init?(coder: NSCoder) { fatalError() }

    private func configureStack() {
        let stack = UIStackView()
        stack.axis = .vertical
        stack.spacing = 5
        stack.translatesAutoresizingMaskIntoConstraints = false
        for line in [label, hint] {
            line.font = .preferredFont(forTextStyle: line === label ? .subheadline : .footnote)
            line.adjustsFontForContentSizeCategory = true
            line.numberOfLines = 0
            line.textColor = .secondaryLabel
        }
        stack.addArrangedSubview(label)
        if kind == "text" {
            label.font = .preferredFont(forTextStyle: .body)
            label.textColor = .label
        }
        let control: UIView
        switch kind {
        case "text": control = hint
        case "segments":
            control = segments
            segments.addTarget(self, action: #selector(segmentChanged), for: .valueChanged)
        case "photo":
            let container = UIView()
            imagePreview.clipsToBounds = true
            imagePreview.layer.cornerRadius = 34
            imagePreview.contentMode = .scaleAspectFill
            imagePreview.translatesAutoresizingMaskIntoConstraints = false
            container.addSubview(imagePreview)
            NSLayoutConstraint.activate([imagePreview.heightAnchor.constraint(equalToConstant: 68), imagePreview.widthAnchor.constraint(equalTo: imagePreview.heightAnchor), imagePreview.leadingAnchor.constraint(equalTo: container.leadingAnchor), imagePreview.topAnchor.constraint(equalTo: container.topAnchor), imagePreview.bottomAnchor.constraint(equalTo: container.bottomAnchor)])
            control = container
        case "menu":
            var config = UIButton.Configuration.plain()
            config.contentInsets = .zero
            config.titleLineBreakMode = .byWordWrapping
            config.image = UIImage(systemName: "chevron.up.chevron.down")
            config.imagePlacement = .trailing
            config.imagePadding = 8
            options.contentHorizontalAlignment = .leading
            options.configuration = config
            options.showsMenuAsPrimaryAction = true
            control = options
        case "search":
            search.searchBarStyle = .minimal
            search.delegate = self
            control = search
        case "multiline", "selectable":
            editor.delegate = self
            editor.backgroundColor = .clear
            editor.font = .preferredFont(forTextStyle: .body)
            editor.adjustsFontForContentSizeCategory = true
            editor.textContainerInset = .zero
            editor.textContainer.lineFragmentPadding = 0
            editor.isScrollEnabled = false
            editor.isEditable = kind != "selectable"
            editor.heightAnchor.constraint(greaterThanOrEqualToConstant: kind == "multiline" ? 68 : 24).isActive = true
            control = editor
        default:
            field.delegate = self
            if standaloneField {
                field.borderStyle = .roundedRect
                field.heightAnchor.constraint(greaterThanOrEqualToConstant: 44).isActive = true
                label.isAccessibilityElement = false
            }
            field.font = .preferredFont(forTextStyle: .body)
            field.adjustsFontForContentSizeCategory = true
            field.isSecureTextEntry = kind == "secure"
            field.addTarget(self, action: #selector(edited), for: .editingChanged)
            field.returnKeyType = .done
            control = field
        }
        if kind != "text" { stack.addArrangedSubview(control) }
        stack.addArrangedSubview(hint)
        contentView.addSubview(stack)
        if standaloneField {
            preservesSuperviewLayoutMargins = false
            contentView.preservesSuperviewLayoutMargins = false
            contentView.directionalLayoutMargins = .zero
        }
        let margins = contentView.layoutMarginsGuide
        NSLayoutConstraint.activate([stack.leadingAnchor.constraint(equalTo: margins.leadingAnchor), stack.trailingAnchor.constraint(equalTo: margins.trailingAnchor), stack.topAnchor.constraint(equalTo: margins.topAnchor), stack.bottomAnchor.constraint(equalTo: margins.bottomAnchor), contentView.heightAnchor.constraint(greaterThanOrEqualToConstant: 44)])
        isAccessibilityElement = false
        selectionStyle = .none
        if kind == "multiline" || kind == "field" || kind == "secure" {
            let bar = UIToolbar()
            let done = UIBarButtonItem(systemItem: .done, primaryAction: UIAction { [weak self] _ in self?.endEditing(true) })
            done.accessibilityLabel = "Dismiss keyboard"
            bar.items = [.flexibleSpace(), done]
            bar.sizeToFit()
            editor.inputAccessoryView = bar
            field.inputAccessoryView = bar
        }
    }

    func bind(_ next: FormRow) {
        let previous = row
        if previous?.id != next.id { hasMarkedEdit = false }
        row = next
        let id = next.identifier.isEmpty ? next.id : next.identifier
        if !stacked {
            var content = defaultContentConfiguration()
            content.text = next.title
            content.secondaryText = next.detail.isEmpty ? nil : next.detail
            content.textProperties.numberOfLines = 0
            content.secondaryTextProperties.numberOfLines = 0
            content.secondaryTextProperties.color = .secondaryLabel
            if kind == "conversation" || kind == "activity" {
                if previous?.value != next.value {
                    avatar = Self.inboxPhoto(next.value)
                }
                let diameter: CGFloat = 44
                content.image = avatar ?? UIImage(systemName: next.symbol.isEmpty ? "person.crop.circle.fill" : next.symbol,
                    withConfiguration: UIImage.SymbolConfiguration(pointSize: 40, weight: .regular))
                let size = CGSize(width: diameter, height: diameter)
                content.imageProperties.reservedLayoutSize = size
                content.imageProperties.maximumSize = size
                content.imageProperties.cornerRadius = diameter / 2
                content.imageProperties.tintColor = .secondaryLabel
                content.textToSecondaryTextVerticalPadding = 3
                content.secondaryTextProperties.numberOfLines = 2
                content.textProperties.font = .preferredFont(forTextStyle: .headline)
                if !next.caption.isEmpty {
                    let detail = NSMutableAttributedString(string: next.detail, attributes: [
                        .font: UIFont.preferredFont(forTextStyle: .subheadline), .foregroundColor: UIColor.label])
                    if !next.detail.isEmpty { detail.append(NSAttributedString(string: "\n")) }
                    detail.append(NSAttributedString(string: next.caption, attributes: [
                        .font: UIFont.preferredFont(forTextStyle: .footnote),
                        .foregroundColor: next.checked ? YapTabBarController.actionTint : UIColor.secondaryLabel]))
                    content.secondaryAttributedText = detail
                    content.secondaryTextProperties.numberOfLines = 0
                }
            } else {
                content.image = next.symbol.isEmpty ? nil : UIImage(systemName: next.symbol)
            }
            content.textProperties.color = !next.enabled ? .tertiaryLabel : next.destructive ? .systemRed : kind == "action" ? YapTabBarController.actionTint : .label
            contentConfiguration = content
            accessoryType = (kind == "disclosure" || kind == "conversation" || kind == "activity") ? .disclosureIndicator : kind == "choice" && next.checked ? .checkmark : .none
            if kind == "toggle" {
                toggle.isOn = next.checked
                toggle.isEnabled = next.enabled
                toggle.accessibilityLabel = next.title
                toggle.accessibilityHint = next.detail
                toggle.accessibilityIdentifier = id
                isAccessibilityElement = false
            } else {
                isAccessibilityElement = true
                accessibilityLabel = next.title
                accessibilityValue = [next.detail, next.caption].filter { !$0.isEmpty }.joined(separator: ", ")
                accessibilityTraits = kind == "text" ? .staticText : .button
                if !next.enabled { accessibilityTraits.insert(.notEnabled) }
                if kind == "choice" && next.checked { accessibilityTraits.insert(.selected) }
                accessibilityIdentifier = id
            }
            selectionStyle = kind == "text" || !next.enabled ? .none : .default
            return
        }
        label.text = next.title
        label.isHidden = next.title.isEmpty || ["menu", "segments", "search", "photo"].contains(kind)
        hint.text = next.detail
        hint.isHidden = next.detail.isEmpty
        switch kind {
        case "text":
            label.accessibilityIdentifier = id
            label.isAccessibilityElement = true
        case "photo":
            if previous?.value != next.value {
                if next.value.isEmpty {
                    imagePreview.image = UIImage(named: "DefaultAvatar")
                } else {
                    imagePreview.image = Data(base64Encoded: next.value).flatMap { UIImage(data: $0) }
                        ?? UIImage(systemName: "person.crop.circle")
                }
            }
            imagePreview.isAccessibilityElement = true
            imagePreview.accessibilityLabel = next.title
            imagePreview.accessibilityIdentifier = id
        case "menu", "segments":
            let choices = next.choices
            if kind == "menu" {
                options.configuration?.title = next.value.isEmpty ? next.title : next.title + " · " + next.value
                options.isEnabled = next.enabled
                options.accessibilityLabel = next.title
                options.accessibilityValue = next.value
                options.accessibilityIdentifier = id
                options.menu = UIMenu(children: choices.enumerated().map { index, choice in
                    UIAction(title: choice.title, attributes: choice.destructive ? .destructive : [], state: choice.selected ? .on : .off) { [weak self] _ in
                        guard let current = self?.row?.choices, current.indices.contains(index), current[index].title == choice.title else { return }
                        current[index].select()
                    }
                })
            } else {
                if previous?.choices.map({ $0.title }) != choices.map({ $0.title }) {
                    segments.removeAllSegments()
                    for (index, choice) in choices.enumerated() { segments.insertSegment(withTitle: choice.title, at: index, animated: false) }
                }
                segments.selectedSegmentIndex = choices.firstIndex(where: { $0.selected }) ?? UISegmentedControl.noSegment
                segments.isEnabled = next.enabled
                segments.accessibilityIdentifier = id
            }
        case "multiline", "selectable":
            if !hasMarkedEdit { syncFormText(editor, displayed: editor.text, value: next.currentText()) }
            editor.accessibilityLabel = next.title
            editor.accessibilityHint = next.placeholder
            editor.accessibilityIdentifier = id
            editor.isEditable = kind == "multiline" && next.enabled
            configureKeyboard(editor, kind: next.keyboard)
        default:
            let input = kind == "search" ? search.searchTextField : field
            if !hasMarkedEdit { syncFormText(input, displayed: input.text ?? "", value: next.currentText()) }
            input.placeholder = next.placeholder.isEmpty ? (kind == "search" ? next.title : nil) : next.placeholder
            input.accessibilityLabel = next.title
            input.accessibilityIdentifier = id
            input.isEnabled = next.enabled
            configureKeyboard(input, kind: next.keyboard)
        }
    }

    private func configureKeyboard(_ control: UIView, kind: String) {
        let keyboard: UIKeyboardType = kind == "number" ? .numberPad : kind.hasPrefix("ascii") ? .asciiCapable : .default
        let caps: UITextAutocapitalizationType = kind == "asciiUpper" ? .allCharacters : (kind.hasPrefix("ascii") || self.kind == "secure" || self.kind == "search") ? .none : .sentences
        let corrections: UITextAutocorrectionType = self.kind == "secure" || !kind.isEmpty ? .no : .default
        if let field = control as? UITextField {
            field.keyboardType = keyboard
            field.autocapitalizationType = caps
            field.autocorrectionType = corrections
            field.spellCheckingType = corrections == .no ? .no : .default
            field.smartQuotesType = corrections == .no ? .no : .default
            field.smartDashesType = corrections == .no ? .no : .default
        } else if let view = control as? UITextView {
            view.keyboardType = keyboard
            view.autocapitalizationType = caps
            view.autocorrectionType = corrections
            view.spellCheckingType = corrections == .no ? .no : .default
            view.smartQuotesType = corrections == .no ? .no : .default
            view.smartDashesType = corrections == .no ? .no : .default
        }
    }
    private static func inboxPhoto(_ encoded: String) -> UIImage? {
        guard !encoded.isEmpty, encoded.utf8.count <= 4096, let data = Data(base64Encoded: encoded),
              let source = CGImageSourceCreateWithData(data as CFData, nil),
              let info = CGImageSourceCopyPropertiesAtIndex(source, 0, nil) as? [CFString: Any],
              let width = info[kCGImagePropertyPixelWidth] as? Int, (1...128).contains(width),
              let height = info[kCGImagePropertyPixelHeight] as? Int, (1...128).contains(height),
              let image = UIImage(data: data) else { return nil }
        // Render a square aspect-fill crop even for profile photos from older clients.
        let size = CGSize(width: 44, height: 44)
        let scale = max(size.width / image.size.width, size.height / image.size.height)
        let scaled = CGSize(width: image.size.width * scale, height: image.size.height * scale)
        return UIGraphicsImageRenderer(size: size).image { _ in
            image.draw(in: CGRect(x: (size.width - scaled.width) / 2, y: (size.height - scaled.height) / 2,
                                  width: scaled.width, height: scaled.height))
        }.withRenderingMode(.alwaysOriginal)
    }

    func activate() {
        guard let row, row.enabled else { return }
        switch kind {
        case "field", "secure": field.becomeFirstResponder()
        case "multiline": editor.becomeFirstResponder()
        case "search": search.searchTextField.becomeFirstResponder()
        case "toggle": row.toggle(selected: !row.checked)
        case "action", "disclosure", "choice", "conversation", "activity": row.tap()
        default: break
        }
    }
    @objc private func switched() { row?.toggle(selected: toggle.isOn) }
    @objc private func segmentChanged() {
        guard let choices = row?.choices, choices.indices.contains(segments.selectedSegmentIndex) else { return }
        choices[segments.selectedSegmentIndex].select()
    }
    private func commitText(_ input: UITextInput, text: String) {
        hasMarkedEdit = input.markedTextRange != nil
        guard !hasMarkedEdit, let row else { return }
        row.edit(text: text)
        syncFormText(input, displayed: text, value: row.currentText())
    }
    @objc private func edited() { commitText(field, text: field.text ?? "") }
    private func accepts(_ value: String, _ range: NSRange, _ replacement: String) -> Bool {
        guard let row else { return false }
        return row.limit <= 0 || value.utf16.count - range.length + replacement.utf16.count <= Int(row.limit)
    }
    func textField(_ textField: UITextField, shouldChangeCharactersIn range: NSRange, replacementString string: String) -> Bool { textField.markedTextRange != nil || accepts(textField.text ?? "", range, string) }
    func textFieldShouldReturn(_ textField: UITextField) -> Bool { textField.resignFirstResponder(); return true }
    func textFieldDidEndEditing(_ textField: UITextField) { if hasMarkedEdit { edited() } }
    func textView(_ textView: UITextView, shouldChangeTextIn range: NSRange, replacementText text: String) -> Bool { textView.markedTextRange != nil || accepts(textView.text, range, text) }
    func textViewDidChange(_ textView: UITextView) {
        commitText(textView, text: textView.text)
        if let table = sequence(first: superview, next: { $0?.superview }).compactMap({ $0 as? UITableView }).first {
            UIView.performWithoutAnimation { table.performBatchUpdates(nil) }
        }
    }
    func textViewDidEndEditing(_ textView: UITextView) { if hasMarkedEdit { commitText(textView, text: textView.text) } }
    func searchBar(_ searchBar: UISearchBar, shouldChangeTextIn range: NSRange, replacementText text: String) -> Bool { searchBar.searchTextField.markedTextRange != nil || accepts(searchBar.text ?? "", range, text) }
    func searchBar(_ searchBar: UISearchBar, textDidChange searchText: String) { commitText(searchBar.searchTextField, text: searchText) }
    func searchBarTextDidEndEditing(_ searchBar: UISearchBar) { if hasMarkedEdit { commitText(searchBar.searchTextField, text: searchBar.text ?? "") } }
    func searchBarSearchButtonClicked(_ searchBar: UISearchBar) { searchBar.resignFirstResponder() }
}

@MainActor
func syncFormText(_ input: UITextInput, displayed: String, value: String) {
    guard displayed != value, input.markedTextRange == nil else { return }
    let selection = input.selectedTextRange.map {
        (input.offset(from: input.beginningOfDocument, to: $0.start),
         input.offset(from: input.beginningOfDocument, to: $0.end))
    }
    if let field = input as? UITextField { field.text = value }
    if let editor = input as? UITextView { editor.text = value }
    guard let (start, end) = selection, (input as? UIView)?.isFirstResponder == true else { return }
    func position(_ offset: Int) -> UITextPosition? {
        let index = offset == displayed.utf16.count ? value.utf16.count : min(offset, value.utf16.count)
        return input.position(from: input.beginningOfDocument, offset: index)
    }
    if let start = position(start), let end = position(end) {
        input.selectedTextRange = input.textRange(from: start, to: end)
    }
}
