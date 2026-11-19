import ImageIO
import PhotosUI
import UniformTypeIdentifiers
import UIKit
import Yappers

/// System photo picker grants access only to the chosen image. No full-library permission.
/// Pass an oriented, bounded local preview to the crop editor.
final class AppleProfilePhotoLibrary: NSObject, ProfilePhotoLibrary, PHPickerViewControllerDelegate {
    private var completion: ((String?, String?) -> Void)?

    func choosePhoto(onResult: @escaping (String?, String?) -> Void) {
        guard completion == nil else { return }
        guard let scene = UIApplication.shared.connectedScenes.compactMap({ $0 as? UIWindowScene }).first(where: { $0.activationState == .foregroundActive }),
              var presenter = scene.windows.first(where: { $0.isKeyWindow })?.rootViewController else {
            onResult(nil, "Couldn't open Photos. Try again.")
            return
        }
        while let presented = presenter.presentedViewController { presenter = presented }
        completion = onResult
        var configuration = PHPickerConfiguration(photoLibrary: .shared())
        configuration.filter = .images
        configuration.selectionLimit = 1
        let picker = PHPickerViewController(configuration: configuration)
        picker.delegate = self
        presenter.present(picker, animated: true)
    }

    func picker(_ picker: PHPickerViewController, didFinishPicking results: [PHPickerResult]) {
        let provider = results.first?.itemProvider
        picker.dismiss(animated: true) { [weak self] in
            guard let provider else { self?.finish(nil, nil); return }
            provider.loadFileRepresentation(forTypeIdentifier: UTType.image.identifier) { [weak self] url, _ in
                let options: [CFString: Any] = [kCGImageSourceCreateThumbnailFromImageAlways: true,
                                                kCGImageSourceCreateThumbnailWithTransform: true,
                                                kCGImageSourceThumbnailMaxPixelSize: 1536]
                guard let url, let source = CGImageSourceCreateWithURL(url as CFURL, nil),
                      let preview = CGImageSourceCreateThumbnailAtIndex(source, 0, options as CFDictionary),
                      let data = UIImage(cgImage: preview).pngData() else {
                    DispatchQueue.main.async { self?.finish(nil, "Couldn’t open this photo. Try another image.") }
                    return
                }
                let encoded = data.base64EncodedString()
                DispatchQueue.main.async { self?.finish(encoded, nil) }
            }
        }
    }

    private func finish(_ photo: String?, _ error: String?) {
        let callback = completion
        completion = nil
        callback?(photo, error)
    }
}
