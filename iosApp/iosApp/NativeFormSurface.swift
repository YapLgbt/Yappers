import UIKit
import Yappers

extension NativeEditorFactory {
    func makeFormSurface(mode: String, onHeight: @escaping (KotlinDouble) -> Void) -> AppleFormSurface {
        NativeFormSurface(mode: mode) { onHeight(KotlinDouble(value: $0)) }
    }

    func showConfirmation(host: UIViewController, confirmation: FormConfirmation) -> UIViewController {
        let alert = formAlert(confirmation)
        presentForm(alert, from: host)
        return alert
    }
}

private func formAlert(_ model: FormConfirmation) -> UIAlertController {
    let alert = UIAlertController(title: model.title, message: model.message, preferredStyle: .alert)
    if !model.cancel.title.isEmpty {
        alert.addAction(UIAlertAction(title: model.cancel.title, style: .cancel) { _ in model.cancel.select() })
    }
    alert.addAction(UIAlertAction(title: model.confirm.title, style: model.confirm.destructive ? .destructive : .default) { _ in
        model.confirm.select()
    })
    alert.view.tintColor = YapTabBarController.actionTint.resolvedColor(with: UITraitCollection(userInterfaceStyle: .light))
    return alert
}

// Route through the presenting hierarchy even when the previous sheet is finishing dismissal.
private func presentForm(_ panel: UIViewController, from host: UIViewController, valid: @escaping () -> Bool = { true }) {
    guard valid() else { return }
    var owner = host.viewIfLoaded?.window?.rootViewController ?? host
    while let next = owner.presentedViewController {
        if next.isBeingDismissed, let transition = next.transitionCoordinator {
            transition.animate(alongsideTransition: nil) { _ in presentForm(panel, from: host, valid: valid) }
            return
        }
        owner = next
    }
    if let transition = owner.transitionCoordinator {
        if transition.animate(alongsideTransition: nil, completion: { _ in presentForm(panel, from: host, valid: valid) }) { return }
    }
    if valid() { owner.present(panel, animated: true) }
}

private final class FormDataSource: UITableViewDiffableDataSource<Int, String> {
    var sections: [FormSection] = []
    override func tableView(_ tableView: UITableView, titleForHeaderInSection section: Int) -> String? {
        guard sections.indices.contains(section) else { return nil }
        let title = sections[section].title
        return title.isEmpty ? nil : title
    }
    override func tableView(_ tableView: UITableView, titleForFooterInSection section: Int) -> String? {
        guard sections.indices.contains(section) else { return nil }
        let text = sections[section].footer
        return text.isEmpty ? nil : text
    }
}

private final class NativeFormSurface: UITableViewController, AppleFormSurface, UIAdaptivePresentationControllerDelegate {
    var controller: UIViewController { self }
    private let mode: String
    private var embedded: Bool { mode == "inline" || mode == "input" }
    private let heightChanged: (Double) -> Void
    private var model: NativeFormModel?
    private var rowModels: [String: FormRow] = [:]
    private var data: FormDataSource!
    private weak var panel: UINavigationController?
    private var sheetRequested = false
    private var height: CGFloat = 0
    private var showingConfirmation = false

    init(mode: String, heightChanged: @escaping (Double) -> Void) {
        self.mode = mode
        self.heightChanged = heightChanged
        super.init(style: (mode == "inline" || mode == "input") ? .plain : .insetGrouped)
    }
    required init?(coder: NSCoder) { fatalError() }

    override func viewDidLoad() {
        super.viewDidLoad()
        tableView.rowHeight = UITableView.automaticDimension
        tableView.estimatedRowHeight = 56
        tableView.keyboardDismissMode = .interactive
        if embedded {
            tableView.isOpaque = false
            tableView.backgroundColor = .clear
            tableView.isScrollEnabled = false
            tableView.contentInsetAdjustmentBehavior = .never
            tableView.sectionHeaderTopPadding = 0
        }
        if mode == "input" {
            tableView.backgroundColor = .secondarySystemFill
            tableView.layer.cornerRadius = 14
            tableView.layer.cornerCurve = .continuous
            tableView.separatorStyle = .none
        }
        data = FormDataSource(tableView: tableView) { [weak self] table, _, key in
            guard let row = self?.rowModels[key] else { return UITableViewCell() }
            let cell = table.dequeueReusableCell(withIdentifier: row.kind) as? NativeFormCell ?? NativeFormCell(kind: row.kind)
            if self?.embedded == true { cell.backgroundColor = .clear }
            cell.bind(row)
            return cell
        }
        data.defaultRowAnimation = .none
        if let model { updateSurface(form: model) }
    }

    func updateSurface(form: NativeFormModel) {
        model = form
        guard isViewLoaded, data != nil else { return }
        navigationItem.title = form.title
        if mode == "sheet" || mode == "compactSheet" {
            configureBar(form.leading, left: true, enabled: form.leadingEnabled)
            configureBar(form.trailing, left: false, enabled: form.trailingEnabled)
        }
        if form.refreshable && refreshControl == nil {
            let control = UIRefreshControl()
            control.addAction(UIAction { [weak self] _ in self?.model?.refresh(); self?.refreshControl?.endRefreshing() }, for: .valueChanged)
            refreshControl = control
        } else if !form.refreshable { refreshControl = nil }

        let previous = data.sections
        data.sections = form.sections
        var snapshot = NSDiffableDataSourceSnapshot<Int, String>()
        var current: [String: FormRow] = [:]
        for (s, section) in form.sections.enumerated() {
            snapshot.appendSections([s])
            var seen: [String: Int] = [:]
            let keys = section.rows.map { row in
                let duplicate = seen[row.id, default: 0]
                seen[row.id] = duplicate + 1
                let key = "\(s)/\(row.kind)/\(duplicate)/\(row.id)"
                current[key] = row
                return key
            }
            snapshot.appendItems(keys, toSection: s)
        }
        rowModels = current
        let old = data.snapshot()
        let headerChanges = snapshot.sectionIdentifiers.filter { index in
            previous.indices.contains(index) && (previous[index].title != form.sections[index].title || previous[index].footer != form.sections[index].footer)
        }
        if old.itemIdentifiers != snapshot.itemIdentifiers || old.sectionIdentifiers != snapshot.sectionIdentifiers || !headerChanges.isEmpty {
            // Updating headers in place keeps the first responder alive while a form changes.
            data.apply(snapshot, animatingDifferences: false)
            for i in headerChanges {
                tableView.headerView(forSection: i)?.textLabel?.text = form.sections[i].title
                tableView.footerView(forSection: i)?.textLabel?.text = form.sections[i].footer
            }
        }
        for path in tableView.indexPathsForVisibleRows ?? [] {
            if let key = data.itemIdentifier(for: path), let row = current[key], let cell = tableView.cellForRow(at: path) as? NativeFormCell {
                cell.bind(row)
            }
        }
        if let confirmation = form.confirmation, !showingConfirmation, view.window != nil {
            showingConfirmation = true
            presentForm(formAlert(confirmation), from: self)
        }
        if form.confirmation == nil { showingConfirmation = false }
        view.setNeedsLayout()
    }

    private func configureBar(_ choice: FormChoice?, left: Bool, enabled: Bool) {
        let item = left ? navigationItem.leftBarButtonItem : navigationItem.rightBarButtonItem
        guard let choice else {
            if left { navigationItem.leftBarButtonItem = nil } else { navigationItem.rightBarButtonItem = nil }
            return
        }
        if item?.title == choice.title { item?.isEnabled = enabled; return }
        let action = UIBarButtonItem(title: choice.title, style: left ? .plain : .done, target: self, action: left ? #selector(leading) : #selector(trailing))
        action.isEnabled = enabled
        if !left { action.tintColor = YapTabBarController.actionTint.resolvedColor(with: UITraitCollection(userInterfaceStyle: .light)) }
        if left { navigationItem.leftBarButtonItem = action } else { navigationItem.rightBarButtonItem = action }
    }
    @objc private func leading() { model?.leading?.select() }
    @objc private func trailing() { model?.trailing?.select() }

    override func viewDidLayoutSubviews() {
        super.viewDidLayoutSubviews()
        if embedded {
            let size = max(1, ceil(tableView.contentSize.height))
            if height != size { height = size; heightChanged(size) }
        }
    }
    override func tableView(_ tableView: UITableView, willDisplayHeaderView view: UIView, forSection section: Int) {
        if embedded { (view as? UITableViewHeaderFooterView)?.backgroundConfiguration = .clear() }
    }
    override func tableView(_ tableView: UITableView, willDisplayFooterView view: UIView, forSection section: Int) {
        if embedded { (view as? UITableViewHeaderFooterView)?.backgroundConfiguration = .clear() }
    }
    override func tableView(_ tableView: UITableView, didSelectRowAt indexPath: IndexPath) {
        tableView.deselectRow(at: indexPath, animated: true)
        guard let cell = tableView.cellForRow(at: indexPath) as? NativeFormCell else { return }
        cell.activate()
    }

    func showSheet(host: UIViewController) {
        sheetRequested = true
        let nav = UINavigationController(rootViewController: self)
        nav.view.tintColor = YapTabBarController.actionTint
        nav.modalPresentationStyle = .pageSheet
        if mode == "compactSheet" { nav.sheetPresentationController?.detents = [.medium(), .large()] }
        nav.sheetPresentationController?.prefersGrabberVisible = true
        nav.presentationController?.delegate = self
        panel = nav
        presentForm(nav, from: host) { [weak self] in self?.sheetRequested == true }
    }
    func closeSheet() {
        sheetRequested = false
        if let panel, !panel.isBeingDismissed { panel.presentingViewController?.dismiss(animated: true) }
    }
    func presentationControllerShouldDismiss(_ presentationController: UIPresentationController) -> Bool {
        model?.canDismiss == true
    }
    func presentationControllerDidAttemptToDismiss(_ presentationController: UIPresentationController) { model?.dismiss() }
    func presentationControllerDidDismiss(_ presentationController: UIPresentationController) { model?.dismiss() }
}
