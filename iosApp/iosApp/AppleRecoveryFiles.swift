import CryptoKit
import Security
import UIKit
import UniformTypeIdentifiers
import Yappers

/// Only user-selected backup files leave/enter the sandbox. No recovery key is sent to a server.
final class AppleRecoveryFiles: NSObject, RecoveryFiles, UIDocumentPickerDelegate {
    private let maximum = 16 * 1024 * 1024 + 65536
    private var completion: ((String?, String?) -> Void)?
    private var exporting = false
    private var temporary: URL?

    func saveRecovery(ciphertext: String, onResult: @escaping (String?, String?) -> Void) {
        guard completion == nil else { onResult(nil, "A file operation is already open."); return }
        completion = onResult
        exporting = true
        DispatchQueue.global(qos: .userInitiated).async { [weak self] in
            guard let self else { return }
            do {
                let input = Array(ciphertext.utf8)
                guard input.count <= self.maximum * 2, input.count % 2 == 0 else { throw self.failure("Backup exceeds file capacity.") }
                func nibble(_ byte: UInt8) -> UInt8? {
                    if byte >= 48 && byte <= 57 { return byte - 48 }
                    if byte >= 97 && byte <= 102 { return byte - 87 }
                    return nil
                }
                var data = Data(capacity: input.count / 2)
                for offset in stride(from: 0, to: input.count, by: 2) {
                    guard let first = nibble(input[offset]), let second = nibble(input[offset + 1]) else { throw self.failure("Invalid encrypted backup.") }
                    data.append(first * 16 + second)
                }
                let directory = FileManager.default.temporaryDirectory.appendingPathComponent(UUID().uuidString, isDirectory: true)
                try FileManager.default.createDirectory(at: directory, withIntermediateDirectories: true, attributes: [.posixPermissions: 0o700])
                let url = directory.appendingPathComponent("Yap-Recovery.age")
                try data.write(to: url, options: [.atomic, .completeFileProtectionUntilFirstUserAuthentication])
                try FileManager.default.setAttributes([.posixPermissions: 0o600], ofItemAtPath: url.path)
                DispatchQueue.main.async {
                    self.temporary = url
                    self.present(UIDocumentPickerViewController(forExporting: [url], asCopy: true))
                }
            } catch { DispatchQueue.main.async { self.finish(nil, "Could not prepare the encrypted backup file.") } }
        }
    }
    func loadRecovery(onResult: @escaping (String?, String?) -> Void) {
        guard completion == nil else { onResult(nil, "A file operation is already open."); return }
        completion = onResult; exporting = false
        present(UIDocumentPickerViewController(forOpeningContentTypes: [.data], asCopy: true))
    }
    private func present(_ picker: UIDocumentPickerViewController) {
        guard let scene = UIApplication.shared.connectedScenes.compactMap({ $0 as? UIWindowScene }).first(where: { $0.activationState == .foregroundActive }),
              var presenter = scene.windows.first(where: { $0.isKeyWindow })?.rootViewController else { finish(nil, "Could not open Files. Try again."); return }
        while let view = presenter.presentedViewController { presenter = view }
        picker.delegate = self; picker.allowsMultipleSelection = false
        presenter.present(picker, animated: true)
    }
    func documentPickerWasCancelled(_ controller: UIDocumentPickerViewController) { finish(nil, nil) }
    func documentPicker(_ controller: UIDocumentPickerViewController, didPickDocumentsAt urls: [URL]) {
        guard let url = urls.first else { finish(nil, nil); return }
        if exporting { finish("saved", nil); return }
        DispatchQueue.global(qos: .userInitiated).async { [weak self] in
            guard let self else { return }
            let access = url.startAccessingSecurityScopedResource()
            defer { if access { url.stopAccessingSecurityScopedResource() } }
            do {
                let size = try url.resourceValues(forKeys: [.fileSizeKey]).fileSize ?? 0
                guard size <= self.maximum else { throw self.failure("Backup exceeds file capacity.") }
                let handle = try FileHandle(forReadingFrom: url)
                defer { try? handle.close() }
                let data = try handle.read(upToCount: self.maximum + 1) ?? Data()
                guard data.count <= self.maximum else { throw self.failure("Backup exceeds file capacity.") }
                let digits = Array("0123456789abcdef".utf8)
                var output = [UInt8](); output.reserveCapacity(data.count * 2)
                for byte in data { output.append(digits[Int(byte >> 4)]); output.append(digits[Int(byte & 15)]) }
                let hex = String(decoding: output, as: UTF8.self)
                DispatchQueue.main.async { self.finish(hex, nil) }
            } catch { DispatchQueue.main.async { self.finish(nil, "Could not read this backup. Choose a Yap .age file within 16 MiB.") } }
        }
    }
    private func failure(_ message: String) -> Error { NSError(domain: "YapRecovery", code: 1, userInfo: [NSLocalizedDescriptionKey: message]) }
    private func finish(_ value: String?, _ error: String?) {
        let callback = completion; completion = nil
        if let temporary {
            try? FileManager.default.removeItem(at: temporary.deletingLastPathComponent())
            self.temporary = nil
        }
        callback?(value, error)
    }
}

extension AppleRecoveryFiles: RecoveryKeyVault {
    // Saved backups are separate from the local sign-in that "Start over" erases.
    private var vaultQuery: [String: Any] {
        let testing = ProcessInfo.processInfo.arguments.contains("--netkit-ui-test")
        return [kSecClass as String: kSecClassGenericPassword,
                kSecAttrService as String: "app.yappers.recovery-key" + (testing ? ".uitests" : ""),
                kSecAttrSynchronizable as String: true]
    }

    private func entryID(_ key: String) -> String {
        SHA256.hash(data: Data(key.utf8)).map { String(format: "%02x", $0) }.joined()
    }

    func hasKey(key: String) -> Bool {
        var query = vaultQuery
        query[kSecAttrAccount as String] = entryID(key)
        return SecItemCopyMatching(query as CFDictionary, nil) == errSecSuccess
    }

    func saveKey(key: String, label: String, onResult: @escaping (String?) -> Void) {
        var item = vaultQuery
        item[kSecAttrAccount as String] = entryID(key)
        item[kSecAttrLabel as String] = label.isEmpty ? "Yap account" : label
        item[kSecAttrAccessible as String] = kSecAttrAccessibleWhenUnlocked
        item[kSecValueData as String] = Data(key.utf8)
        let status = SecItemAdd(item as CFDictionary, nil)
        onResult(status == errSecSuccess || status == errSecDuplicateItem ? nil : "Couldn’t save to Keychain. You can still copy your key.")
    }

    func removeKey(key: String, onResult: @escaping (String?) -> Void) {
        var query = vaultQuery
        query[kSecAttrAccount as String] = entryID(key)
        let status = SecItemDelete(query as CFDictionary)
        onResult(status == errSecSuccess || status == errSecItemNotFound ? nil : "Couldn’t remove the saved key. Try again.")
    }

    func chooseKey(onResult: @escaping (String?, String?) -> Void) {
        var query = vaultQuery
        query[kSecReturnAttributes as String] = true
        query[kSecMatchLimit as String] = kSecMatchLimitAll
        var result: CFTypeRef?
        let status = SecItemCopyMatching(query as CFDictionary, &result)
        guard status == errSecSuccess, let items = result as? [[String: Any]], !items.isEmpty else {
            onResult(nil, status == errSecItemNotFound ? "No Yap recovery keys in this Keychain yet." : "Couldn’t open Keychain. Try again.")
            return
        }
        func choose(_ item: [String: Any]) {
            guard let id = item[kSecAttrAccount as String] as? String else { onResult(nil, "Couldn’t read this key."); return }
            var lookup = vaultQuery
            lookup[kSecAttrAccount as String] = id
            lookup[kSecReturnData as String] = true
            var data: CFTypeRef?
            guard SecItemCopyMatching(lookup as CFDictionary, &data) == errSecSuccess,
                  let bytes = data as? Data, let key = String(data: bytes, encoding: .utf8) else {
                onResult(nil, "Couldn’t read this key. Try again."); return
            }
            onResult(key, nil)
        }
        if items.count == 1 { choose(items[0]); return }
        let selector = UIAlertController(title: "Choose a Yap account", message: nil, preferredStyle: .alert)
        for item in items {
            let label = item[kSecAttrLabel as String] as? String ?? "Yap account"
            let date = item[kSecAttrCreationDate as String] as? Date
            let title = date.map { label + " · " + $0.formatted(date: .abbreviated, time: .shortened) } ?? label
            selector.addAction(UIAlertAction(title: title, style: .default) { _ in choose(item) })
        }
        selector.addAction(UIAlertAction(title: "Cancel", style: .cancel) { _ in onResult(nil, nil) })
        guard let scene = UIApplication.shared.connectedScenes.compactMap({ $0 as? UIWindowScene }).first(where: { $0.activationState == .foregroundActive }),
              var presenter = scene.windows.first(where: { $0.isKeyWindow })?.rootViewController else {
            onResult(nil, "Couldn’t open the account picker. Try again."); return
        }
        while let next = presenter.presentedViewController { presenter = next }
        presenter.present(selector, animated: true)
    }
}
