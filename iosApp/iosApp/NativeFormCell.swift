import UIKit
import Yappers

final class NativeFormCell: UITableViewCell, UITextFieldDelegate, UITextViewDelegate, UISearchBarDelegate {
    private let kind: String
    private var row: FormRow?
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

    init(kind: String) {
        self.kind = kind
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
        let margins = contentView.layoutMarginsGuide
        NSLayoutConstraint.activate([stack.leadingAnchor.constraint(equalTo: margins.leadingAnchor), stack.trailingAnchor.constraint(equalTo: margins.trailingAnchor), stack.topAnchor.constraint(equalTo: margins.topAnchor), stack.bottomAnchor.constraint(equalTo: margins.bottomAnchor), contentView.heightAnchor.constraint(greaterThanOrEqualToConstant: 44)])
        isAccessibilityElement = false
        selectionStyle = .none
        if kind == "multiline" || kind == "field" || kind == "secure" {
            let bar = UIToolbar()
            let done = UIBarButtonItem(systemItem: .done, primaryAction: UIAction { [weak self] _ in self?.endEditing(true) })
            done.accessibilityLabel = "Dismiss keyboard"
            done.tintColor = YapTabBarController.actionTint.resolvedColor(with: UITraitCollection(userInterfaceStyle: .light))
            bar.items = [.flexibleSpace(), done]
            bar.sizeToFit()
            editor.inputAccessoryView = bar
            field.inputAccessoryView = bar
        }
    }

    func bind(_ next: FormRow) {
        let previous = row
        row = next
        let id = next.identifier.isEmpty ? next.id : next.identifier
        if !stacked {
            var content = defaultContentConfiguration()
            content.text = next.title
            content.secondaryText = next.detail.isEmpty ? nil : next.detail
            content.textProperties.numberOfLines = 0
            content.secondaryTextProperties.numberOfLines = 0
            content.secondaryTextProperties.color = .secondaryLabel
            content.image = next.symbol.isEmpty ? nil : UIImage(systemName: next.symbol)
            content.textProperties.color = !next.enabled ? .tertiaryLabel : next.destructive ? .systemRed : kind == "action" ? YapTabBarController.actionTint : .label
            contentConfiguration = content
            accessoryType = kind == "disclosure" ? .disclosureIndicator : kind == "choice" && next.checked ? .checkmark : .none
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
                accessibilityValue = next.detail.isEmpty ? nil : next.detail
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
            if previous?.value != next.value { imagePreview.image = Data(base64Encoded: next.value).flatMap { UIImage(data: $0) } }
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
            if editor.markedTextRange == nil && editor.text != next.value && (!editor.isFirstResponder || previous?.value != next.value) { editor.text = next.value }
            editor.accessibilityLabel = next.title
            editor.accessibilityHint = next.placeholder
            editor.accessibilityIdentifier = id
            editor.isEditable = kind == "multiline" && next.enabled
            configureKeyboard(editor, kind: next.keyboard)
        default:
            let input = kind == "search" ? search.searchTextField : field
            if input.markedTextRange == nil && input.text != next.value && (!input.isFirstResponder || previous?.value != next.value) { input.text = next.value }
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
    func activate() {
        guard let row, row.enabled else { return }
        switch kind {
        case "field", "secure": field.becomeFirstResponder()
        case "multiline": editor.becomeFirstResponder()
        case "search": search.searchTextField.becomeFirstResponder()
        case "toggle": row.toggle(selected: !row.checked)
        case "action", "disclosure", "choice": row.tap()
        default: break
        }
    }
    @objc private func switched() { row?.toggle(selected: toggle.isOn) }
    @objc private func segmentChanged() {
        guard let choices = row?.choices, choices.indices.contains(segments.selectedSegmentIndex) else { return }
        choices[segments.selectedSegmentIndex].select()
    }
    @objc private func edited() { row?.edit(text: field.text ?? "") }
    private func accepts(_ value: String, _ range: NSRange, _ replacement: String) -> Bool {
        guard let row else { return false }
        return row.limit <= 0 || value.utf16.count - range.length + replacement.utf16.count <= Int(row.limit)
    }
    func textField(_ textField: UITextField, shouldChangeCharactersIn range: NSRange, replacementString string: String) -> Bool { accepts(textField.text ?? "", range, string) }
    func textFieldShouldReturn(_ textField: UITextField) -> Bool { textField.resignFirstResponder(); return true }
    func textView(_ textView: UITextView, shouldChangeTextIn range: NSRange, replacementText text: String) -> Bool { accepts(textView.text, range, text) }
    func textViewDidChange(_ textView: UITextView) {
        row?.edit(text: textView.text)
        if let table = sequence(first: superview, next: { $0?.superview }).compactMap({ $0 as? UITableView }).first {
            UIView.performWithoutAnimation { table.performBatchUpdates(nil) }
        }
    }
    func searchBar(_ searchBar: UISearchBar, shouldChangeTextIn range: NSRange, replacementText text: String) -> Bool { accepts(searchBar.text ?? "", range, text) }
    func searchBar(_ searchBar: UISearchBar, textDidChange searchText: String) { row?.edit(text: searchText) }
    func searchBarSearchButtonClicked(_ searchBar: UISearchBar) { searchBar.resignFirstResponder() }
}
