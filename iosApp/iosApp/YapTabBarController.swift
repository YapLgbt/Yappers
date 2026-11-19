import UIKit
import Yappers

final class YapTabBarController: UITabBarController, UITabBarControllerDelegate, UINavigationControllerDelegate {
    private enum Destination: String, CaseIterable {
        case nearby = "Nearby"
        case groups = "Groups"
        case saved = "Saved"
        case you = "You"

        var symbol: String {
            switch self {
            case .nearby: "bubble.left.and.bubble.right"
            case .groups: "person.2"
            case .saved: "bookmark"
            case .you: "person.crop.circle"
            }
        }
    }

    private let session: AppleAppSession
    private var stacks: [Destination: UINavigationController] = [:]
    private var rootBars: [Destination: (tabs: Bool, toolbar: Bool)] = [:]

    static let actionTint = UIColor { traits in
        if traits.userInterfaceStyle == .dark {
            return UIColor(red: 215 / 255, green: 181 / 255, blue: 1, alpha: 1)
        }
        return UIColor(red: 105 / 255, green: 67 / 255, blue: 148 / 255, alpha: 1)
    }

    static let doneItemTint: UIColor = {
        if #available(iOS 26.0, *) {
            // UIKit fills prominent Done items with their tint on iOS 26.
            return actionTint.resolvedColor(with: UITraitCollection(userInterfaceStyle: .light))
        }
        return actionTint
    }()

    init(session: AppleAppSession) {
        self.session = session
        super.init(nibName: nil, bundle: nil)
    }

    @available(*, unavailable)
    required init?(coder: NSCoder) { fatalError("Use init(session:)") }

    deinit { session.close() }

    override func viewDidLoad() {
        super.viewDidLoad()
        view.tintColor = Self.actionTint
        delegate = self
        viewControllers = Destination.allCases.map(makeStack)
        selectTab(session.initialTab)
    }

    private func makeStack(_ destination: Destination) -> UINavigationController {
        let content = session.makeTab(tab: destination.rawValue, onSelectTab: { [weak self] tab in
            self?.selectTab(tab, returningToRoot: true)
        }, onSetRootBars: { [weak self] tabs, toolbar in
            guard let self else { return }
            self.rootBars[destination] = (tabs.boolValue, toolbar.boolValue)
            self.applyRootBars(for: destination)
        }, onToolbar: { [weak self] actions in
            self?.updateToolbar(actions, for: destination)
        }, onOpenProfileSettings: { [weak self] in
            self?.openProfileSettings()
        }, onOpenPrivateGroups: { [weak self] in
            self?.openPrivateGroups(in: destination)
        }, onOpenInbox: { [weak self] page, contact in
            self?.openInbox(page, contact: contact, in: destination)
        }, onOpenPost: { [weak self] route in
            self?.openPost(route, in: destination)
        })

        let item = content.navigationItem
        item.title = destination.rawValue
        item.backButtonDisplayMode = .minimal
        let wordmark = UIImageView(image: UIImage(named: "YapWordmark"))
        wordmark.tintColor = .label
        wordmark.isAccessibilityElement = true
        wordmark.accessibilityLabel = "Yap"
        wordmark.accessibilityTraits = .header
        item.titleView = wordmark
        item.leftBarButtonItem = UIBarButtonItem(primaryAction: UIAction(title: "Inbox", image: UIImage(systemName: "tray")) { [weak self] _ in
            self?.openInbox(.messages, contact: nil, in: destination)
        })
        item.leftBarButtonItem?.accessibilityIdentifier = "Inbox"
        item.leftBarButtonItem?.accessibilityLabel = "Inbox"

        let navigation = UINavigationController(rootViewController: content)
        navigation.setNavigationBarHidden(!session.tabsVisible, animated: false)
        navigation.delegate = self
        navigation.tabBarItem = UITabBarItem(title: destination.rawValue, image: UIImage(systemName: destination.symbol),
                                            selectedImage: UIImage(systemName: destination.symbol + ".fill"))
        stacks[destination] = navigation
        return navigation
    }

    private func updateToolbar(_ actions: AppleToolbarActions, for destination: Destination) {
        guard let item = stacks[destination]?.viewControllers.first?.navigationItem else { return }
        if destination != .groups {
            let post = UIBarButtonItem(primaryAction: UIAction(title: "Post", image: UIImage(systemName: "square.and.pencil")) { _ in
                actions.post()
            })
            post.accessibilityLabel = "Post"
            post.isEnabled = actions.postEnabled
            item.rightBarButtonItem = post
        }
        let count = actions.inboxCount
        item.leftBarButtonItem?.image = count == 0 ? UIImage(systemName: "tray") : nil
        item.leftBarButtonItem?.title = count == 0 ? nil : "Inbox (\(count))"
        item.leftBarButtonItem?.accessibilityLabel = count == 0 ? "Inbox" : "Inbox, \(count) unread"
        item.titleView?.accessibilityCustomActions = actions.refreshLabel.map { label in
            [UIAccessibilityCustomAction(name: label) { _ in actions.refresh(); return true }]
        }
    }

    private func selectTab(_ title: String, returningToRoot: Bool = false) {
        guard let destination = Destination(rawValue: title), let navigation = stacks[destination] else { return }
        if returningToRoot { navigation.popToRootViewController(animated: false) }
        selectedViewController = navigation
        session.didSelectTab(tab: title)
        applyRootBars(for: destination)
    }

    private func openPrivateGroups(in destination: Destination, group: String? = nil, title: String = "Private groups") {
        guard destination == .groups, let navigation = stacks[destination],
              navigation === selectedViewController, navigation.transitionCoordinator == nil else { return }
        let screen = session.makePrivateGroups(group: group, onOpenGroup: { [weak self] id, name in
            self?.openPrivateGroups(in: destination, group: id, title: name)
        }, onClose: { [weak navigation] in
            navigation?.popToRootViewController(animated: false)
        })
        screen.navigationItem.title = title
        screen.navigationItem.backButtonDisplayMode = .minimal
        screen.hidesBottomBarWhenPushed = true
        navigation.pushViewController(screen, animated: true)
    }

    private func openProfileSettings() {
        guard let navigation = stacks[.you], navigation === selectedViewController,
              navigation.viewControllers.count == 1, navigation.transitionCoordinator == nil else { return }
        let content = session.makeProfileSettings(onSelectTab: { [weak self, weak navigation] tab in
            navigation?.popToRootViewController(animated: false)
            self?.selectTab(tab, returningToRoot: true)
        }, onOpenGuide: { [weak self] in
            self?.openGuide()
        }, onOpenInbox: { [weak self] page, contact in
            self?.openInbox(page, contact: contact, in: .you)
        })
        content.navigationItem.title = "Settings"
        content.navigationItem.backButtonDisplayMode = .minimal
        navigation.pushViewController(content, animated: true)
    }

    private func openGuide() {
        guard let navigation = stacks[.you], navigation === selectedViewController,
              navigation.transitionCoordinator == nil, presentedViewController == nil else { return }
        let content = session.makeGuide(onClose: { [weak self] in
            self?.dismiss(animated: true)
        }, onDone: { [weak self, weak navigation] in
            self?.dismiss(animated: true) {
                navigation?.popToRootViewController(animated: false)
                self?.selectTab("Nearby", returningToRoot: true)
            }
        })
        content.modalPresentationStyle = .fullScreen
        present(content, animated: true)
    }

    private func openPost(_ route: ApplePostRoute, in destination: Destination) {
        guard let navigation = stacks[destination], selectedViewController === navigation,
              navigation.viewControllers.count == 1, navigation.transitionCoordinator == nil else { return }
        let content = session.makePost(tab: destination.rawValue, route: route, onOpenInbox: { [weak self] page, contact in
            self?.openInbox(page, contact: contact, in: destination)
        }, onClose: { [weak navigation] in
            navigation?.popViewController(animated: true)
        })
        content.navigationItem.title = route.title
        content.navigationItem.largeTitleDisplayMode = .never
        content.hidesBottomBarWhenPushed = true
        navigation.pushViewController(content, animated: true)
    }

    private func openInbox(_ page: InboxPage, contact: MessageContact?, in destination: Destination) {
        guard let navigation = stacks[destination], navigation === selectedViewController,
              navigation.transitionCoordinator == nil else { return }
        let content = session.makeInbox(tab: destination.rawValue, page: page, contact: contact, onSelectTab: { [weak self, weak navigation] tab in
            navigation?.popToRootViewController(animated: false)
            self?.selectTab(tab, returningToRoot: true)
        }, onOpenInbox: { [weak self] nextPage, nextContact in
            self?.openInbox(nextPage, contact: nextContact, in: destination)
        }, onOpenProfileSettings: { [weak self] in
            self?.openProfileSettings()
        }, onOpenPost: { [weak self] route in self?.openPost(route, in: destination) }, onCloseStack: { [weak navigation] in
            navigation?.popToRootViewController(animated: false)
        })
        let item = content.navigationItem
        item.backButtonDisplayMode = .minimal
        switch page {
        case .messages:
            item.title = "Inbox"
            let settings = [("Requests", InboxPage.requests), ("Privacy", .privacy), ("Blocked accounts", .blocked)]
            let menu = UIMenu(children: settings.map { title, page in
                UIAction(title: title) { [weak self] _ in self?.openInbox(page, contact: nil, in: destination) }
            })
            item.rightBarButtonItem = UIBarButtonItem(image: UIImage(systemName: "ellipsis"), menu: menu)
            item.rightBarButtonItem?.accessibilityLabel = "Inbox settings"
        case .requests: item.title = "Requests"
        case .blocked: item.title = "Blocked accounts"
        case .privacy: item.title = "Privacy"
        default: item.title = contact?.label ?? "Conversation"
        }
        content.hidesBottomBarWhenPushed = navigation.topViewController?.hidesBottomBarWhenPushed == true
        navigation.pushViewController(content, animated: true)
    }

    private func applyRootBars(for destination: Destination) {
        guard let navigation = stacks[destination], navigation === selectedViewController,
              navigation.viewControllers.count == 1 else { return }
        let bars = rootBars[destination]
        setTabBarHidden(!(bars?.tabs ?? session.tabsVisible), animated: false)
        navigation.setNavigationBarHidden(!(bars?.toolbar ?? session.tabsVisible), animated: false)
    }

    func tabBarController(_ tabBarController: UITabBarController, didSelect viewController: UIViewController) {
        guard let destination = stacks.first(where: { $0.value === viewController })?.key else { return }
        session.didSelectTab(tab: destination.rawValue)
        applyRootBars(for: destination)
    }

    func navigationController(_ navigationController: UINavigationController, willShow viewController: UIViewController, animated: Bool) {
        guard let destination = stacks.first(where: { $0.value === navigationController })?.key else { return }
        let root = viewController === navigationController.viewControllers.first
        navigationController.setNavigationBarHidden(root && !(rootBars[destination]?.toolbar ?? session.tabsVisible), animated: animated)
    }

    func navigationController(_ navigationController: UINavigationController, didShow viewController: UIViewController, animated: Bool) {
        guard navigationController === selectedViewController,
              viewController === navigationController.viewControllers.first,
              let destination = stacks.first(where: { $0.value === navigationController })?.key else { return }
        applyRootBars(for: destination)
    }
}
