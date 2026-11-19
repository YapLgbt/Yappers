import UIKit
import Yappers

final class NativeEditorFactory: NSObject, AppleEditorFactory {
    func makePostForm(model: PostForm) -> ApplePostFormView {
        NativePostForm(model: model)
    }
}

private final class WritingCell: UITableViewCell {
    let editor = UITextView()
    private let count = UILabel()
    private let placeholder = UILabel()

    init(title: String, prompt: String, identifier: String, font: UIFont, lines: CGFloat) {
        super.init(style: .default, reuseIdentifier: nil)
        selectionStyle = .none
        isAccessibilityElement = false
        let label = UILabel()
        label.text = title
        label.font = .preferredFont(forTextStyle: .subheadline)
        label.adjustsFontForContentSizeCategory = true
        label.textColor = .secondaryLabel
        label.numberOfLines = 0
        count.font = .preferredFont(forTextStyle: .caption1)
        count.adjustsFontForContentSizeCategory = true
        count.textColor = .secondaryLabel
        count.textAlignment = .natural
        count.setContentHuggingPriority(.required, for: .horizontal)
        let labels = UIStackView(arrangedSubviews: [label, count])
        labels.spacing = 8
        labels.alignment = .firstBaseline
        editor.font = font
        editor.adjustsFontForContentSizeCategory = true
        editor.backgroundColor = .clear
        editor.isScrollEnabled = false
        editor.textContainerInset = .zero
        editor.textContainer.lineFragmentPadding = 0
        editor.accessibilityLabel = title
        editor.accessibilityHint = prompt
        editor.accessibilityIdentifier = identifier

        placeholder.text = prompt
        placeholder.font = font
        placeholder.adjustsFontForContentSizeCategory = true
        placeholder.textColor = .placeholderText
        placeholder.numberOfLines = 0
        placeholder.isAccessibilityElement = false
        placeholder.translatesAutoresizingMaskIntoConstraints = false
        editor.addSubview(placeholder)

        let stack = UIStackView(arrangedSubviews: [labels, editor])
        stack.axis = .vertical
        stack.spacing = 8
        stack.translatesAutoresizingMaskIntoConstraints = false
        contentView.addSubview(stack)
        let margins = contentView.layoutMarginsGuide
        NSLayoutConstraint.activate([
            stack.topAnchor.constraint(equalTo: margins.topAnchor),
            stack.leadingAnchor.constraint(equalTo: margins.leadingAnchor),
            stack.trailingAnchor.constraint(equalTo: margins.trailingAnchor),
            stack.bottomAnchor.constraint(equalTo: margins.bottomAnchor),
            editor.heightAnchor.constraint(greaterThanOrEqualToConstant: max(44, lines * font.lineHeight)),
            placeholder.topAnchor.constraint(equalTo: editor.topAnchor),
            placeholder.leadingAnchor.constraint(equalTo: editor.leadingAnchor),
            placeholder.widthAnchor.constraint(equalTo: editor.widthAnchor),
            editor.heightAnchor.constraint(greaterThanOrEqualTo: placeholder.heightAnchor)
        ])
    }

    required init?(coder: NSCoder) { fatalError() }

    func updateCount(limit: Int32) {
        let length = editor.text.utf16.count
        count.text = "\(length) / \(limit)"
        placeholder.isHidden = length > 0
    }
}

private final class NativePostForm: UITableViewController, ApplePostFormView, UITextViewDelegate {
    var controller: UIViewController { self }
    private var model: PostForm
    private let headline = WritingCell(title: "Headline", prompt: "Give your thought a little title", identifier: "post-headline-input",
                                       font: NativeWritingType.publicHeadline, lines: 1)
    private let body = WritingCell(title: "Body", prompt: "What's on your mind, neighbor?", identifier: "post-body-input",
                                   font: NativeWritingType.publicBody, lines: 4)
    private let identity = UIButton(type: .system)
    private let topic = UIButton(type: .system)
    private let claim = UIButton(type: .system)
    private lazy var identityCell = actionCell(identity)
    private lazy var topicCell = actionCell(topic)
    private lazy var claimCell = actionCell(claim)
    private var claimVisible: Bool { !model.anonymous && model.usernameClaim != nil }

    init(model: PostForm) {
        self.model = model
        super.init(style: .insetGrouped)
    }

    required init?(coder: NSCoder) { fatalError() }

    override func viewDidLoad() {
        super.viewDidLoad()
        tableView.keyboardDismissMode = .interactive
        tableView.rowHeight = UITableView.automaticDimension
        tableView.estimatedRowHeight = 66
        let accessory = UIToolbar()
        accessory.tintColor = YapTabBarController.actionTint.resolvedColor(with: UITraitCollection(userInterfaceStyle: .light))
        let done = UIBarButtonItem(systemItem: .done, primaryAction: UIAction { [weak self] _ in self?.view.endEditing(true) })
        done.accessibilityLabel = "Dismiss keyboard"
        accessory.items = [.flexibleSpace(), done]
        accessory.sizeToFit()
        for cell in [headline, body] {
            cell.editor.delegate = self
            cell.editor.inputAccessoryView = accessory
        }
        headline.editor.text = model.draft.headline
        body.editor.text = model.draft.body
        identity.showsMenuAsPrimaryAction = true
        topic.showsMenuAsPrimaryAction = true
        configure(claim, title: "Choose a username")
        claim.addAction(UIAction { [weak self] _ in
            guard let self, let request = self.model.usernameClaim else { return }
            NativeUsernameClaim.present(from: self, claim: request)
        }, for: .primaryActionTriggered)
        updateIdentity()
        updateTopic()
        updateWriting()
    }

    private func actionCell(_ button: UIButton) -> UITableViewCell {
        let cell = UITableViewCell(style: .default, reuseIdentifier: nil)
        cell.isAccessibilityElement = false
        cell.selectionStyle = .none
        button.contentHorizontalAlignment = .leading
        button.translatesAutoresizingMaskIntoConstraints = false
        cell.contentView.addSubview(button)
        let content = cell.contentView
        NSLayoutConstraint.activate([
            button.leadingAnchor.constraint(equalTo: content.leadingAnchor), button.trailingAnchor.constraint(equalTo: content.trailingAnchor),
            button.topAnchor.constraint(equalTo: content.topAnchor), button.bottomAnchor.constraint(equalTo: content.bottomAnchor),
            button.heightAnchor.constraint(greaterThanOrEqualToConstant: 48)
        ])
        return cell
    }

    private func configure(_ button: UIButton, title: String, menu: Bool = false) {
        var config = UIButton.Configuration.plain()
        config.title = title
        config.titleLineBreakMode = .byWordWrapping
        config.contentInsets = .init(top: 10, leading: 16, bottom: 10, trailing: 16)
        if menu {
            config.image = UIImage(systemName: "chevron.up.chevron.down")
            config.preferredSymbolConfigurationForImage = .init(textStyle: .caption1)
            config.imagePlacement = .trailing
            config.imagePadding = 8
        }
        button.configuration = config
    }

    private func updateIdentity() {
        let title = "Post as " + (model.anonymous ? "Anonymous" : model.namedIdentity)
        configure(identity, title: title, menu: true)
        let actions = [(true, "Anonymous"), (false, model.namedIdentity)].map { anonymous, name in
            let action = UIAction(title: name, state: model.anonymous == anonymous ? .on : .off) { [weak self] _ in
                self?.model.selectIdentity(anonymous: anonymous)
            }
            if !anonymous { action.accessibilityIdentifier = "named-posting-identity" }
            return action
        }
        identity.menu = UIMenu(options: .singleSelection, children: actions)
    }

    private func updateTopic() {
        let draft = model.draft
        configure(topic, title: "Topic · " + draft.category, menu: true)
        topic.accessibilityLabel = "Topic"
        topic.accessibilityValue = draft.category
        topic.menu = UIMenu(options: .singleSelection, children: draft.topicChoices.map { name in
            UIAction(title: name, state: name == draft.category ? .on : .off) { [weak self] _ in
                self?.model.draft.selectTopic(topic: name)
                self?.updateTopic()
            }
        })
    }

    private func updateWriting() {
        headline.updateCount(limit: model.draft.headlineLimit)
        body.updateCount(limit: model.draft.bodyLimit)
    }

    func renderForm(model next: PostForm) {
        let previous = model
        let previouslyClaimable = claimVisible
        model = next
        guard isViewLoaded else { return }
        if previous.anonymous != next.anonymous || previous.namedIdentity != next.namedIdentity { updateIdentity() }
        if previouslyClaimable != claimVisible || previous.destination != next.destination || previous.networked != next.networked {
            tableView.reloadSections(IndexSet(integer: 0), with: .none)
        }
        if previous.submitting != next.submitting {
            for field in [headline.editor, body.editor] { field.isEditable = !next.submitting }
            for button in [identity, claim, topic] { button.isEnabled = !next.submitting }
        }
        if let error = next.error, presentedViewController == nil {
            let alert = UIAlertController(title: "Couldn't save that", message: error, preferredStyle: .alert)
            alert.addAction(UIAlertAction(title: "OK", style: .default) { [weak self] _ in self?.model.dismissError() })
            alert.view.tintColor = view.tintColor
            present(alert, animated: true)
        }
    }

    override func numberOfSections(in tableView: UITableView) -> Int { 3 }

    override func tableView(_ tableView: UITableView, numberOfRowsInSection section: Int) -> Int {
        switch section {
        case 0: return claimVisible ? 2 : 1
        case 1: return 2
        default: return 1
        }
    }

    override func tableView(_ tableView: UITableView, titleForHeaderInSection section: Int) -> String? {
        section == 0 ? "Posting to " + model.destination : nil
    }

    override func tableView(_ tableView: UITableView, titleForFooterInSection section: Int) -> String? {
        section == 0 && model.networked ? "Your home can identify anonymous posts." : nil
    }

    override func tableView(_ tableView: UITableView, cellForRowAt path: IndexPath) -> UITableViewCell {
        switch path.section {
        case 0: return path.row == 0 ? identityCell : claimCell
        case 1: return path.row == 0 ? headline : body
        default: return topicCell
        }
    }

    override func tableView(_ tableView: UITableView, didSelectRowAt path: IndexPath) {
        tableView.deselectRow(at: path, animated: false)
        guard path.section == 1, !model.submitting else { return }
        let field = path.row == 0 ? headline.editor : body.editor
        field.becomeFirstResponder()
    }

    func textView(_ textView: UITextView, shouldChangeTextIn range: NSRange, replacementText text: String) -> Bool {
        let limit = textView === headline.editor ? model.draft.headlineLimit : model.draft.bodyLimit
        return !model.submitting && textView.text.utf16.count - range.length + text.utf16.count <= Int(limit)
    }

    func textViewDidChange(_ textView: UITextView) {
        let draft = model.draft
        let accepted: String
        if textView === headline.editor {
            draft.editHeadline(text: textView.text)
            accepted = draft.headline
        } else {
            draft.editBody(text: textView.text)
            accepted = draft.body
        }
        // Dictation and text services may bypass the proposed-replacement delegate.
        if textView.markedTextRange == nil && textView.text != accepted { textView.text = accepted }
        updateWriting()
        UIView.performWithoutAnimation { tableView.performBatchUpdates(nil) }
    }
}
