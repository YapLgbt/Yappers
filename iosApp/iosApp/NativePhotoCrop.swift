import UIKit
import Yappers

extension NativeEditorFactory {
    func makePhotoCrop(model: NativePhotoCrop) -> UIViewController { ProfilePhotoCrop(model) }
}

private final class ProfilePhotoCrop: UIViewController, UIScrollViewDelegate {
    private let crop: NativePhotoCrop
    private let viewport = UIScrollView()
    private let photo = UIImageView()
    private let zoom = UISlider()
    private var previousSide: CGFloat = 0

    init(_ crop: NativePhotoCrop) {
        self.crop = crop
        super.init(nibName: nil, bundle: nil)
    }
    required init?(coder: NSCoder) { fatalError() }

    override func viewDidLoad() {
        super.viewDidLoad()
        view.backgroundColor = .systemBackground
        view.tintColor = YapTabBarController.actionTint
        navigationItem.title = "Crop photo"
        navigationItem.leftBarButtonItem = UIBarButtonItem(systemItem: .cancel, primaryAction: UIAction { [weak self] _ in self?.crop.cancel() })
        navigationItem.rightBarButtonItem = UIBarButtonItem(title: "Use crop", style: .done, target: self, action: #selector(save))
        navigationItem.rightBarButtonItem?.tintColor = YapTabBarController.doneItemTint

        let size = CGSize(width: Int(crop.width), height: Int(crop.height))
        if let data = Data(base64Encoded: crop.preview) { photo.image = UIImage(data: data) }
        photo.frame = CGRect(origin: .zero, size: size)
        viewport.contentSize = size
        viewport.delegate = self
        viewport.contentInsetAdjustmentBehavior = .never
        viewport.showsHorizontalScrollIndicator = false
        viewport.showsVerticalScrollIndicator = false
        viewport.bouncesZoom = false
        viewport.bounces = false
        viewport.backgroundColor = .white
        viewport.clipsToBounds = true
        viewport.isAccessibilityElement = true
        viewport.accessibilityLabel = "Profile photo crop"
        viewport.accessibilityHint = "Drag to move your photo. Pinch to zoom."
        viewport.accessibilityTraits = .image
        viewport.accessibilityCustomActions = [("Move photo left", 1, 0), ("Move photo right", -1, 0),
                                               ("Move photo up", 0, 1), ("Move photo down", 0, -1)].map { name, dx, dy in
            UIAccessibilityCustomAction(name: name) { [weak self] _ in
                guard let self else { return false }
                let delta = self.viewport.bounds.width * 0.1
                self.move(to: CGPoint(x: self.viewport.contentOffset.x + CGFloat(dx) * delta,
                                      y: self.viewport.contentOffset.y + CGFloat(dy) * delta))
                return true
            }
        }
        viewport.addSubview(photo)
        let hint = UILabel()
        hint.text = "Drag to move your photo. Pinch to zoom."
        hint.font = .preferredFont(forTextStyle: .body)
        hint.adjustsFontForContentSizeCategory = true
        hint.numberOfLines = 0
        hint.textAlignment = .center
        hint.textColor = .secondaryLabel

        zoom.minimumValue = 1
        zoom.maximumValue = 4
        zoom.accessibilityLabel = "Zoom"
        zoom.addTarget(self, action: #selector(changeZoom), for: .valueChanged)
        let guide = view.safeAreaLayoutGuide
        [viewport, hint, zoom].forEach { $0.translatesAutoresizingMaskIntoConstraints = false; view.addSubview($0) }
        let preferred = viewport.widthAnchor.constraint(equalToConstant: 320)
        preferred.priority = .defaultHigh
        NSLayoutConstraint.activate([
            viewport.centerXAnchor.constraint(equalTo: guide.centerXAnchor), viewport.topAnchor.constraint(equalTo: guide.topAnchor, constant: 20),
            viewport.widthAnchor.constraint(lessThanOrEqualTo: guide.widthAnchor, constant: -40), preferred,
            viewport.heightAnchor.constraint(equalTo: viewport.widthAnchor),
            hint.topAnchor.constraint(equalTo: viewport.bottomAnchor, constant: 16),
            hint.leadingAnchor.constraint(equalTo: guide.leadingAnchor, constant: 20), hint.trailingAnchor.constraint(equalTo: guide.trailingAnchor, constant: -20),
            zoom.topAnchor.constraint(equalTo: hint.bottomAnchor, constant: 12), zoom.leadingAnchor.constraint(equalTo: viewport.leadingAnchor),
            zoom.trailingAnchor.constraint(equalTo: viewport.trailingAnchor), zoom.heightAnchor.constraint(greaterThanOrEqualToConstant: 44),
            zoom.bottomAnchor.constraint(lessThanOrEqualTo: guide.bottomAnchor, constant: -12)
        ])
    }

    override func viewDidLayoutSubviews() {
        super.viewDidLayoutSubviews()
        let side = viewport.bounds.width
        guard side > 0, side != previousSide else { return }
        let previousScale = viewport.zoomScale
        let center = previousSide > 0 ? CGPoint(x: (viewport.contentOffset.x + previousSide / 2) / previousScale,
                                                y: (viewport.contentOffset.y + previousSide / 2) / previousScale) :
            CGPoint(x: photo.bounds.midX, y: photo.bounds.midY)
        let relative = previousSide > 0 ? previousScale / viewport.minimumZoomScale : 1
        let fit = side / min(photo.bounds.width, photo.bounds.height)
        viewport.minimumZoomScale = fit
        viewport.maximumZoomScale = fit * 4
        viewport.zoomScale = fit * relative
        viewport.layer.cornerRadius = side / 2
        move(to: CGPoint(x: center.x * viewport.zoomScale - side / 2, y: center.y * viewport.zoomScale - side / 2))
        previousSide = side
    }

    private func move(to point: CGPoint) {
        let x = min(max(0, point.x), max(0, viewport.contentSize.width - viewport.bounds.width))
        let y = min(max(0, point.y), max(0, viewport.contentSize.height - viewport.bounds.height))
        viewport.contentOffset = CGPoint(x: x, y: y)
    }

    func viewForZooming(in scrollView: UIScrollView) -> UIView? { photo }
    func scrollViewDidZoom(_ scrollView: UIScrollView) {
        zoom.value = Float(scrollView.zoomScale / scrollView.minimumZoomScale)
        zoom.accessibilityValue = String(format: "%.1f×", zoom.value)
    }
    @objc private func changeZoom() {
        viewport.setZoomScale(viewport.minimumZoomScale * CGFloat(zoom.value), animated: false)
    }
    @objc private func save() {
        let visible = photo.convert(viewport.bounds, from: viewport)
        if let message = crop.useCrop(left: visible.minX, top: visible.minY, side: visible.width) {
            let alert = UIAlertController(title: "Couldn’t save this crop", message: message, preferredStyle: .alert)
            alert.addAction(UIAlertAction(title: "OK", style: .default))
            present(alert, animated: true)
        }
    }
}
