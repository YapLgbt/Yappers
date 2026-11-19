import UIKit
import Yappers

final class NativeUsernameClaim: UITableViewController, UITextFieldDelegate {
    static func present(from presenter: UIViewController, claim: EditorUsernameClaim) {
        guard presenter.presentedViewController == nil else { return }
        let form = NativeUsernameClaim(claim)
        let navigation = UINavigationController(rootViewController: form)
        navigation.view.tintColor = YapTabBarController.actionTint
        navigation.modalPresentationStyle = .pageSheet
        presenter.present(navigation, animated: true)
    }

    private let claim: EditorUsernameClaim
    private let field = UITextField()
    private var busy = false

    private init(_ claim: EditorUsernameClaim) {
        self.claim = claim
        super.init(style: .insetGrouped)
    }

    @available(*, unavailable)
    required init?(coder: NSCoder) { fatalError("Use init(_:)") }

    override func viewDidLoad() {
        super.viewDidLoad()
        title = "Choose a username"
        navigationItem.leftBarButtonItem = UIBarButtonItem(systemItem: .cancel, primaryAction: UIAction { [weak self] _ in
            self?.dismiss(animated: true)
        })
        navigationItem.rightBarButtonItem = UIBarButtonItem(title: "Claim", style: .done, target: self, action: #selector(submit))
        navigationItem.rightBarButtonItem?.accessibilityLabel = "Claim username"

        field.accessibilityIdentifier = "editor-claim-username"
        field.accessibilityLabel = "Username"
        field.placeholder = "Username"
        field.font = .preferredFont(forTextStyle: .body)
        field.adjustsFontForContentSizeCategory = true
        field.autocapitalizationType = .none
        field.autocorrectionType = .no
        field.spellCheckingType = .no
        field.keyboardType = .asciiCapable
        field.returnKeyType = .done
        field.delegate = self
        field.addTarget(self, action: #selector(edited), for: .editingChanged)
        edited()
    }

    override func numberOfSections(in tableView: UITableView) -> Int { 1 }
    override func tableView(_ tableView: UITableView, numberOfRowsInSection section: Int) -> Int { 1 }

    override func tableView(_ tableView: UITableView, cellForRowAt indexPath: IndexPath) -> UITableViewCell {
        let cell = UITableViewCell(style: .default, reuseIdentifier: nil)
        cell.selectionStyle = .none
        cell.contentView.addSubview(field)
        field.translatesAutoresizingMaskIntoConstraints = false
        let margins = cell.contentView.layoutMarginsGuide
        NSLayoutConstraint.activate([
            field.leadingAnchor.constraint(equalTo: margins.leadingAnchor),
            field.trailingAnchor.constraint(equalTo: margins.trailingAnchor),
            field.topAnchor.constraint(equalTo: margins.topAnchor),
            field.bottomAnchor.constraint(equalTo: margins.bottomAnchor),
            field.heightAnchor.constraint(greaterThanOrEqualToConstant: 30)
        ])
        return cell
    }

    override func tableView(_ tableView: UITableView, titleForFooterInSection section: Int) -> String? {
        "Your permanent address ends in @\(claim.domain) and appears on named posts and replies. Use 2–24 letters, numbers, or underscores. Start with a letter."
    }

    @objc private func edited() {
        guard field.markedTextRange == nil else {
            navigationItem.rightBarButtonItem?.isEnabled = false
            return
        }
        let text = field.text ?? ""
        let name = claim.normalize(name: text)
        syncFormText(field, displayed: text, value: name)
        navigationItem.rightBarButtonItem?.isEnabled = !busy && claim.isValid(name: name)
    }

    func textFieldShouldReturn(_ textField: UITextField) -> Bool {
        if claim.isValid(name: textField.text ?? "") { submit() }
        return false
    }

    @objc private func submit() {
        guard !busy, field.markedTextRange == nil, let name = field.text, claim.isValid(name: name) else { return }
        busy = true
        navigationController?.isModalInPresentation = true
        field.isEnabled = false
        navigationItem.leftBarButtonItem?.isEnabled = false
        edited()
        claim.submit(name: name) { [weak self] error in
            guard let self else { return }
            guard let error else {
                self.dismiss(animated: true)
                return
            }
            self.busy = false
            self.navigationController?.isModalInPresentation = false
            self.field.isEnabled = true
            self.navigationItem.leftBarButtonItem?.isEnabled = true
            self.edited()
            let alert = UIAlertController(title: "Couldn’t claim username", message: error, preferredStyle: .alert)
            alert.addAction(UIAlertAction(title: "OK", style: .default))
            self.present(alert, animated: true)
        }
    }
}
