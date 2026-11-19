import UIKit
import Yappers

extension NativeEditorFactory {
    func makeFeedControl() -> AppleFeedControl { NativeFeedButton() }
}

private final class NativeFeedButton: UIView, AppleFeedControl {
    var view: UIView { self }
    private let button = UIButton(type: .system)
    private lazy var disclosure: UIImageView = {
        let icon = UIImageView(image: UIImage(systemName: "chevron.forward", withConfiguration: UIImage.SymbolConfiguration(textStyle: .footnote, scale: .small)))
        icon.tintColor = .tertiaryLabel
        icon.translatesAutoresizingMaskIntoConstraints = false
        button.addSubview(icon)
        NSLayoutConstraint.activate([icon.trailingAnchor.constraint(equalTo: button.trailingAnchor, constant: -4),
                                     icon.centerYAnchor.constraint(equalTo: button.centerYAnchor)])
        return icon
    }()
    private var current: FormRow?

    init() {
        super.init(frame: .zero)
        addSubview(button)
        button.autoresizingMask = [.flexibleWidth, .flexibleHeight]
        button.addAction(UIAction { [weak self] _ in self?.current?.tap() }, for: .touchUpInside)
    }
    required init?(coder: NSCoder) { fatalError() }

    func configureControl(row: FormRow) {
        let wasMethod = current?.kind == "method"
        current = row
        let story = row.kind == "story"
        let byline = row.kind == "byline"
        let compact = row.kind == "icon" || row.kind == "menu"
        let method = row.kind == "method"
        if method || wasMethod { disclosure.isHidden = !method }
        button.accessibilityIdentifier = row.identifier.isEmpty ? nil : row.identifier
        button.accessibilityLabel = byline ? "View \(row.title)'s profile" : story ? [row.title, row.detail].filter { !$0.isEmpty }.joined(separator: ". ") : row.title
        button.accessibilityValue = (row.kind == "location" || byline) && !row.detail.isEmpty ? row.detail : nil
        var traits: UIAccessibilityTraits = row.kind == "link" ? .link : .button
        if row.checked { traits.insert(.selected) }
        if !row.enabled { traits.insert(.notEnabled) }
        button.accessibilityTraits = traits
        button.tintColor = story ? .label : nil
        button.contentHorizontalAlignment = story || byline || method || row.kind == "location" ? .leading : .center
        button.accessibilityHint = method ? row.detail : nil
        button.isEnabled = row.enabled
        if row.kind == "primary" || row.kind == "secondary" {
            let primary = row.kind == "primary"
            var configuration = primary ? UIButton.Configuration.filled() : .plain()
            configuration.title = row.title
            if !row.symbol.isEmpty {
                configuration.image = UIImage(systemName: row.symbol)
                configuration.imagePadding = 6
            }
            if primary { configuration.baseForegroundColor = .systemBackground }
            configuration.buttonSize = primary ? .large : .medium
            configuration.cornerStyle = .large
            configuration.titleLineBreakMode = .byWordWrapping
            button.configuration = configuration
        } else if story {
            button.configuration = nil
            button.titleLabel?.numberOfLines = 0
            button.titleLabel?.lineBreakMode = .byWordWrapping
            let text = NSMutableAttributedString()
            if !row.title.isEmpty {
                let paragraph = NSMutableParagraphStyle()
                paragraph.paragraphSpacing = 6
                text.append(NSAttributedString(string: row.title + "\n", attributes: [.font: NativeWritingType.publicHeadline, .foregroundColor: UIColor.label, .paragraphStyle: paragraph]))
            }
            text.append(NSAttributedString(string: row.detail, attributes: [.font: NativeWritingType.publicBody, .foregroundColor: UIColor.label]))
            button.setAttributedTitle(text, for: .normal)
        } else {
            var configuration = UIButton.Configuration.plain()
            configuration.title = compact ? nil : row.title
            if !row.detail.isEmpty { configuration.subtitle = row.detail }
            configuration.titleAlignment = .leading
            configuration.titleLineBreakMode = .byWordWrapping
            configuration.subtitleLineBreakMode = .byWordWrapping
            configuration.buttonSize = .small
            configuration.contentInsets = NSDirectionalEdgeInsets(top: 0, leading: 0, bottom: 0, trailing: 0)
            if !row.symbol.isEmpty {
                let small = UIImage.SymbolConfiguration(textStyle: compact ? .subheadline : .body)
                configuration.image = UIImage(systemName: row.symbol, withConfiguration: small)
                configuration.imagePadding = 4
            }
            if method {
                let iconColumnWidth = UIFont.preferredFont(forTextStyle: .body).lineHeight * 1.65
                configuration.imagePadding = 12 + max(0, iconColumnWidth - (configuration.image?.size.width ?? 0))
                configuration.titlePadding = 4
                configuration.contentInsets = NSDirectionalEdgeInsets(top: 14, leading: 2, bottom: 14, trailing: 26)
                configuration.titleTextAttributesTransformer = UIConfigurationTextAttributesTransformer { incoming in
                    var text = incoming
                    text.font = .preferredFont(forTextStyle: .headline)
                    text.foregroundColor = .label
                    return text
                }
                configuration.subtitleTextAttributesTransformer = UIConfigurationTextAttributesTransformer { incoming in
                    var text = incoming
                    text.font = .preferredFont(forTextStyle: .subheadline)
                    text.foregroundColor = .secondaryLabel
                    return text
                }
            } else if row.kind == "location" {
                configuration.imagePlacement = .trailing
                configuration.titleTextAttributesTransformer = UIConfigurationTextAttributesTransformer { incoming in
                    var updated = incoming
                    updated.font = .preferredFont(forTextStyle: .title2).withTraits(.traitBold)
                    return updated
                }
                configuration.baseForegroundColor = .label
            } else if row.destructive {
                configuration.baseForegroundColor = .systemRed
            } else if compact && !row.checked {
                configuration.baseForegroundColor = .secondaryLabel
            }
            button.configuration = configuration
        }
        if !row.choices.isEmpty {
            button.menu = UIMenu(children: row.choices.map { choice in
                UIAction(title: choice.title, attributes: choice.destructive ? .destructive : [], state: choice.selected ? .on : .off) { _ in choice.select() }
            })
            button.showsMenuAsPrimaryAction = true
        } else {
            button.menu = nil
            button.showsMenuAsPrimaryAction = false
        }
    }
    override func sizeThatFits(_ size: CGSize) -> CGSize {
        let available = max(1, size.width)
        if current?.kind == "story" {
            let bounds = button.attributedTitle(for: .normal)?.boundingRect(
                with: CGSize(width: available, height: .greatestFiniteMagnitude),
                options: [.usesLineFragmentOrigin, .usesFontLeading], context: nil) ?? .zero
            return CGSize(width: available, height: max(44, ceil(bounds.height)))
        }
        let size = button.sizeThatFits(CGSize(width: available, height: .greatestFiniteMagnitude))
        let width = current?.kind == "primary" || current?.kind == "method" ? available : min(available, max(44, ceil(size.width)))
        return CGSize(width: width, height: max(44, ceil(size.height)))
    }

}

private extension UIFont {
    func withTraits(_ traits: UIFontDescriptor.SymbolicTraits) -> UIFont {
        guard let descriptor = fontDescriptor.withSymbolicTraits(traits) else { return self }
        return UIFont(descriptor: descriptor, size: pointSize)
    }
}

extension NativeEditorFactory {
    func makeRefreshIndicator() -> AppleRefreshIndicator { FeedPullIndicator() }

    func showTransientNotice(host: UIViewController, message: String) {
        guard let window = host.viewIfLoaded?.window else { return }
        window.subviews.compactMap { $0 as? FeedNotice }.forEach { $0.removeFromSuperview() }
        let notice = FeedNotice(message: message)
        window.addSubview(notice)
        let bottom = host.tabBarController.flatMap { $0.isTabBarHidden ? nil : $0.tabBar.topAnchor }
            ?? window.safeAreaLayoutGuide.bottomAnchor
        NSLayoutConstraint.activate([
            notice.centerXAnchor.constraint(equalTo: window.safeAreaLayoutGuide.centerXAnchor),
            notice.leadingAnchor.constraint(greaterThanOrEqualTo: window.safeAreaLayoutGuide.leadingAnchor, constant: 16),
            notice.trailingAnchor.constraint(lessThanOrEqualTo: window.safeAreaLayoutGuide.trailingAnchor, constant: -16),
            notice.bottomAnchor.constraint(equalTo: bottom, constant: -12)
        ])
        UIAccessibility.post(notification: .announcement, argument: message)
        DispatchQueue.main.asyncAfter(deadline: .now() + (UIAccessibility.isVoiceOverRunning ? 6 : 3)) { [weak notice] in
            notice?.removeFromSuperview()
        }
    }
}

private final class FeedPullIndicator: UIView, AppleRefreshIndicator {
    var view: UIView { self }
    private let arrow = UIImageView(image: UIImage(systemName: "arrow.down"))
    private let spinner = UIActivityIndicatorView(style: .medium)

    init() {
        super.init(frame: .zero)
        isUserInteractionEnabled = false
        accessibilityElementsHidden = true
        arrow.tintColor = .secondaryLabel
        arrow.contentMode = .center
        for component in [arrow as UIView, spinner] {
            component.frame = bounds
            component.autoresizingMask = [.flexibleWidth, .flexibleHeight]
            addSubview(component)
        }
    }

    @available(*, unavailable)
    required init?(coder: NSCoder) { fatalError() }

    func updatePull(progress: Double, refreshing: Bool) {
        arrow.isHidden = refreshing
        arrow.transform = UIAccessibility.isReduceMotionEnabled ? .identity : CGAffineTransform(rotationAngle: .pi * progress)
        if refreshing && !spinner.isAnimating {
            spinner.startAnimating()
            UIAccessibility.post(notification: .announcement, argument: "Refreshing")
        } else if !refreshing && spinner.isAnimating {
            spinner.stopAnimating()
        }
    }
}

private final class FeedNotice: UIView {
    init(message: String) {
        super.init(frame: .zero)
        accessibilityElementsHidden = true
        isUserInteractionEnabled = false
        translatesAutoresizingMaskIntoConstraints = false
        backgroundColor = .secondarySystemBackground
        layer.cornerCurve = .continuous
        layer.cornerRadius = 12
        let label = UILabel()
        label.text = message
        label.numberOfLines = 0
        label.textAlignment = .center
        label.font = .preferredFont(forTextStyle: .subheadline)
        label.adjustsFontForContentSizeCategory = true
        label.translatesAutoresizingMaskIntoConstraints = false
        addSubview(label)
        NSLayoutConstraint.activate([
            label.leadingAnchor.constraint(equalTo: leadingAnchor, constant: 16),
            label.trailingAnchor.constraint(equalTo: trailingAnchor, constant: -16),
            label.topAnchor.constraint(equalTo: topAnchor, constant: 10),
            label.bottomAnchor.constraint(equalTo: bottomAnchor, constant: -10)
        ])
    }

    @available(*, unavailable)
    required init?(coder: NSCoder) { fatalError() }
}
