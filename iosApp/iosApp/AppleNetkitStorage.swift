import CryptoKit
import Foundation
import Security
import Yappers

/// Keys/capabilities never enter UserDefaults. The outbox is an atomic protected file.
final class AppleNetkitStorage: NSObject, NetkitStorage {
    private static let accountService = "app.yappers.prototype.netkit.v1"
    private let deletionJournal = "account-deletion-v1"
    private let service: String
    private let directoryName: String
    var stopAccountActivity: (() -> Void)?
    override init() {
        #if DEBUG
        let testing = ProcessInfo.processInfo.arguments.contains("--netkit-ui-test")
        #else
        let testing = false
        #endif
        service = Self.accountService + (testing ? ".uitests" : "")
        directoryName = testing ? "NetkitUITests" : "Netkit"
        super.init()
        #if DEBUG
        if testing && (ProcessInfo.processInfo.arguments.contains("--netkit-reset") || ProcessInfo.processInfo.arguments.contains("--simulate-reinstall")) {
            UserDefaults.standard.removeObject(forKey: "yap.welcome.test.complete")
            UserDefaults.standard.removeObject(forKey: "yappers.prototype.test.v1")
            if let url = try? stateURL { try? FileManager.default.removeItem(at: url.deletingLastPathComponent()) }
        }
        if testing && ProcessInfo.processInfo.arguments.contains("--netkit-reset") {
            let query: [String: Any] = [kSecClass as String: kSecClassGenericPassword, kSecAttrService as String: service]
            SecItemDelete(query as CFDictionary)
            SecItemDelete([kSecClass as String: kSecClassGenericPassword, kSecAttrService as String: service + ".deletion"] as CFDictionary)
            SecItemDelete([kSecClass as String: kSecClassGenericPassword,
                           kSecAttrService as String: "app.yappers.recovery-key.uitests",
                           kSecAttrSynchronizable as String: true] as CFDictionary)
        }
        #endif
    }
    #if DEBUG
    /// A developer-invoked local reset, paired with uninstall to clear the app container.
    static func clearAccountKeychainForFirstRun() throws {
        for name in [accountService, accountService + ".deletion", accountService + ".uitests", accountService + ".uitests.deletion"] {
            let query: [String: Any] = [
                kSecClass as String: kSecClassGenericPassword,
                kSecAttrService as String: name,
                kSecAttrSynchronizable as String: false
            ]
            let deletion = SecItemDelete(query as CFDictionary)
            guard deletion == errSecSuccess || deletion == errSecItemNotFound else {
                throw NSError(domain: NSOSStatusErrorDomain, code: Int(deletion))
            }
            let verification = SecItemCopyMatching(query as CFDictionary, nil)
            guard verification == errSecItemNotFound else {
                throw NSError(domain: "YapLocalResetVerification", code: Int(verification))
            }
        }
    }
    #endif

    /// Called at cold start, before a controller or background sync can open the store.
    func eraseLocalAccount() throws {
        let directory = try stateURL.deletingLastPathComponent()
        try FileManager.default.removeItem(at: directory)
        let query: [String: Any] = [kSecClass as String: kSecClassGenericPassword, kSecAttrService as String: service]
        let status = SecItemDelete(query as CFDictionary)
        guard status == errSecSuccess || status == errSecItemNotFound else {
            throw NSError(domain: NSOSStatusErrorDomain, code: Int(status))
        }
        let defaults = UserDefaults.standard
        let testing = service.hasSuffix(".uitests")
        defaults.removeObject(forKey: testing ? "yappers.prototype.test.v1" : "yappers.prototype.v1")
        defaults.removeObject(forKey: testing ? "yap.welcome.test.complete" : "yap.welcome.complete")
        if !testing {
            let files = FileManager.default
            // Clean up copies left by interrupted exports. User-selected Files destinations are outside this directory.
            for item in try files.contentsOfDirectory(at: files.temporaryDirectory, includingPropertiesForKeys: nil)
                where UUID(uuidString: item.lastPathComponent) != nil {
                let copy = item.appendingPathComponent("Yap-Recovery.age")
                if files.fileExists(atPath: copy.path) { try files.removeItem(at: copy) }
            }
        }
    }

    func eraseDeletedAccount(recoveryKey: String) throws {
        // Pending signed batches may be large; they normally share the protected
        // file store. Keep the compact confirmation independently through cleanup.
        let confirmed = try readSecret(key: deletionJournal)
        if confirmed != "null" { try saveKeychain(deletionJournal, confirmed, namespace: service + ".deletion") }
        if let stopAccountActivity {
            if Thread.isMainThread { stopAccountActivity() }
            else { DispatchQueue.main.sync(execute: stopAccountActivity) }
        }
        try AppleRecoveryFiles.eraseSavedKey(recoveryKey, testing: service.hasSuffix(".uitests"))
        try eraseLocalAccount()
        guard UserDefaults.standard.synchronize() else { throw CocoaError(.fileWriteUnknown) }
        if !service.hasSuffix(".uitests") {
            // Older document pickers imported sandbox copies of selected files.
            let files = FileManager.default
            let documents = try files.url(for: .documentDirectory, in: .userDomainMask, appropriateFor: nil, create: false)
            let inbox = documents.appendingPathComponent("Inbox")
            if files.fileExists(atPath: inbox.path) { try files.removeItem(at: inbox) }
            for item in try files.contentsOfDirectory(at: files.temporaryDirectory, includingPropertiesForKeys: nil)
                where item.lastPathComponent.hasSuffix("-Inbox") { try files.removeItem(at: item) }
        }
        let query: [String: Any] = [kSecClass as String: kSecClassGenericPassword,
            kSecAttrService as String: service + ".deletion", kSecAttrAccount as String: deletionJournal]
        let status = SecItemDelete(query as CFDictionary)
        guard status == errSecSuccess || status == errSecItemNotFound else {
            throw NSError(domain: NSOSStatusErrorDomain, code: Int(status))
        }
    }

    private var stateURL: URL {
        get throws {
            let base = try FileManager.default.url(for: .applicationSupportDirectory, in: .userDomainMask, appropriateFor: nil, create: true)
            var directory = base.appendingPathComponent(directoryName, isDirectory: true)
            try FileManager.default.createDirectory(at: directory, withIntermediateDirectories: true,
                attributes: [.protectionKey: FileProtectionType.completeUntilFirstUserAuthentication, .posixPermissions: 0o700])
            var values = URLResourceValues(); values.isExcludedFromBackup = true
            try directory.setResourceValues(values)
            return directory.appendingPathComponent("state-v1.json")
        }
    }
    func readState() throws -> String {
        let url = try stateURL
        guard FileManager.default.fileExists(atPath: url.path) else { return "null" }
        return try String(contentsOf: url, encoding: .utf8)
    }
    func writeState(value: String) throws {
        let url = try stateURL
        try Data(value.utf8).write(to: url, options: [.atomic, .completeFileProtectionUntilFirstUserAuthentication])
        let file = try FileHandle(forWritingTo: url)
        defer { try? file.close() }
        try file.synchronize()
    }
    private func conversationURL(_ name: String) throws -> URL? {
        if name == deletionJournal { return try stateURL.deletingLastPathComponent().appendingPathComponent(name + ".sealed") }
        guard name.hasPrefix("direct-v1-") else { return nil }
        guard name.range(of: #"^direct-v1-[0-9a-f]{64}-[0-9a-f]{64}-[0-9a-f]{64}$"#, options: .regularExpression) != nil else {
            throw CocoaError(.fileReadInvalidFileName)
        }
        return try stateURL.deletingLastPathComponent().appendingPathComponent(name + ".sealed")
    }
    private func conversationKey(create: Bool) throws -> SymmetricKey {
        let name = "message-file-key"
        let stored = try keychainValue(name)
        if let bytes = Data(base64Encoded: stored), bytes.count == 32 {
            return SymmetricKey(data: bytes)
        }
        guard create && stored == "null" else { throw CocoaError(.fileReadCorruptFile) }
        let key = SymmetricKey(size: .bits256)
        try saveKeychain(name, key.withUnsafeBytes { Data($0).base64EncodedString() })
        return key
    }
    func readSecret(key: String) throws -> String {
        if key == deletionJournal {
            let confirmed = try keychainValue(key, namespace: service + ".deletion")
            if confirmed != "null" { return confirmed }
        }
        guard let url = try conversationURL(key) else { return try keychainValue(key) }
        guard FileManager.default.fileExists(atPath: url.path) else { return "null" }
        let box = try AES.GCM.SealedBox(combined: Data(contentsOf: url))
        let bytes = try AES.GCM.open(box, using: conversationKey(create: false), authenticating: Data(key.utf8))
        return String(decoding: bytes, as: UTF8.self)
    }
    func writeSecret(key: String, value: String) throws {
        guard let url = try conversationURL(key) else { return try saveKeychain(key, value) }
        let fileKey = try conversationKey(create: !FileManager.default.fileExists(atPath: url.path))
        let box = try AES.GCM.seal(Data(value.utf8), using: fileKey, authenticating: Data(key.utf8))
        guard let bytes = box.combined else { throw CocoaError(.fileWriteUnknown) }
        try bytes.write(to: url, options: [.atomic, .completeFileProtectionUntilFirstUserAuthentication])
        let file = try FileHandle(forWritingTo: url)
        defer { try? file.close() }
        try file.synchronize()
    }
    private func keychainValue(_ key: String, namespace: String? = nil) throws -> String {
        let query: [String: Any] = [kSecClass as String: kSecClassGenericPassword,
            kSecAttrService as String: namespace ?? service, kSecAttrAccount as String: key,
            kSecReturnData as String: true, kSecMatchLimit as String: kSecMatchLimitOne]
        var result: CFTypeRef?
        let status = SecItemCopyMatching(query as CFDictionary, &result)
        if status == errSecItemNotFound { return "null" }
        guard status == errSecSuccess, let data = result as? Data, let value = String(data: data, encoding: .utf8) else {
            throw NSError(domain: "YappersKeychain", code: Int(status), userInfo: [NSLocalizedDescriptionKey: "Cannot read the saved network identity or connection. Unlock the device and retry."])
        }
        return value
    }
    private func saveKeychain(_ key: String, _ value: String, namespace: String? = nil) throws {
        let query: [String: Any] = [kSecClass as String: kSecClassGenericPassword,
            kSecAttrService as String: namespace ?? service, kSecAttrAccount as String: key]
        let update: [String: Any] = [kSecValueData as String: Data(value.utf8),
            kSecAttrAccessible as String: kSecAttrAccessibleAfterFirstUnlockThisDeviceOnly]
        var status = SecItemUpdate(query as CFDictionary, update as CFDictionary)
        if status == errSecItemNotFound {
            status = SecItemAdd(query.merging(update) { _, new in new } as CFDictionary, nil)
        }
        guard status == errSecSuccess else {
            throw NSError(domain: "YappersKeychain", code: Int(status), userInfo: [NSLocalizedDescriptionKey: "Cannot save the network identity or connection. Unlock the device and retry."])
        }
    }
}
