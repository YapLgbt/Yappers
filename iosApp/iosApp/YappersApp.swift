import BackgroundTasks
import OSLog
import SwiftUI
import UserNotifications
import Yappers

private final class AppleInboxPush: NSObject, PushRegistration {
    let provider = "apns"
    private(set) var currentToken: String?
    private var completion: ((String?, String?) -> Void)?
    private var generation = UUID()
    private var suppressionKey: String {
        #if DEBUG
        if ProcessInfo.processInfo.arguments.contains("--netkit-ui-test") { return "yap.pushDeleted.uitests" }
        #endif
        return "yap.pushDeleted"
    }
    var enabled: Bool { !UserDefaults.standard.bool(forKey: suppressionKey) }

    func clearAccount() {
        generation = UUID()
        UserDefaults.standard.set(true, forKey: suppressionKey)
        UIApplication.shared.unregisterForRemoteNotifications()
        UNUserNotificationCenter.current().removeAllPendingNotificationRequests()
        UNUserNotificationCenter.current().removeAllDeliveredNotifications()
        UNUserNotificationCenter.current().setBadgeCount(0)
        finish(token: nil, error: "Account deleted.")
    }

    func requestToken(completion: @escaping (String?, String?) -> Void) {
        DispatchQueue.main.async {
            UserDefaults.standard.removeObject(forKey: self.suppressionKey)
            let generation = self.generation
            self.completion = completion
            UNUserNotificationCenter.current().requestAuthorization(options: [.alert, .sound, .badge]) { allowed, _ in
                DispatchQueue.main.async {
                    guard self.generation == generation else { return }
                    if allowed { UIApplication.shared.registerForRemoteNotifications() }
                    else { self.finish(token: nil, error: "Allow notifications for Yap in Settings.") }
                }
            }
        }
    }

    func finish(token: String?, error: String?) {
        currentToken = enabled ? token : nil
        let callback = completion
        completion = nil
        callback?(currentToken, error)
    }
}

private final class AppDelegate: NSObject, UIApplicationDelegate, UNUserNotificationCenterDelegate, ObservableObject {
    private let inboxPush = AppleInboxPush()

    func application(_ application: UIApplication, didRegisterForRemoteNotificationsWithDeviceToken deviceToken: Data) {
        inboxPush.finish(token: deviceToken.map { String(format: "%02x", $0) }.joined(), error: nil)
    }

    func application(_ application: UIApplication, didFailToRegisterForRemoteNotificationsWithError error: Error) {
        inboxPush.finish(token: nil, error: "Couldn’t register for notifications. Try again.")
    }

    func userNotificationCenter(_ center: UNUserNotificationCenter, didReceive response: UNNotificationResponse,
                                withCompletionHandler completionHandler: @escaping () -> Void) {
        if inboxPush.enabled, let route = response.notification.request.content.userInfo["route"] as? String {
            InboxPush.shared.open(route: route)
        }
        completionHandler()
    }

    func userNotificationCenter(_ center: UNUserNotificationCenter, willPresent notification: UNNotification,
                                withCompletionHandler completionHandler: @escaping (UNNotificationPresentationOptions) -> Void) {
        completionHandler(inboxPush.enabled ? [.banner, .list] : [])
    }

    private let identifier = "app.yappers.prototype.refresh"
    private let logger = Logger(subsystem: "app.yappers.prototype", category: "BackgroundSync")
    private static var resetMarker: String {
        #if DEBUG
        if ProcessInfo.processInfo.arguments.contains("--netkit-ui-test") { return "yap.resetInProgress.uitests" }
        #endif
        return "yap.resetInProgress"
    }
    @Published private(set) var reviewingReset: Bool = {
        if UserDefaults.standard.bool(forKey: AppDelegate.resetMarker) { return true }
        #if DEBUG
        if ProcessInfo.processInfo.arguments.contains("--review-device-reset") { return true }
        if ProcessInfo.processInfo.arguments.contains("--netkit-ui-test") { return false }
        #endif
        return UserDefaults.standard.bool(forKey: "yap.offerDeviceReset")
    }()
    var resetStarted: Bool { UserDefaults.standard.bool(forKey: Self.resetMarker) }

    func finishResetReview() {
        #if DEBUG
        let testing = ProcessInfo.processInfo.arguments.contains("--netkit-ui-test")
        #else
        let testing = false
        #endif
        if !testing { UserDefaults.standard.removeObject(forKey: "yap.offerDeviceReset") }
        UserDefaults.standard.removeObject(forKey: Self.resetMarker)
        reviewingReset = false
    }

    func eraseData() throws {
        precondition(reviewingReset)
        UserDefaults.standard.set(true, forKey: Self.resetMarker)
        cancelScheduled()
        try AppleNetkitStorage().eraseLocalAccount()
        finishResetReview()
    }

    lazy var nearby = AppleNearbyServices()
    lazy var runtime: AppleSyncRuntime = {
        let store = AppleNetkitStorage()
        store.stopAccountActivity = { [weak self] in
            guard let self else { return }
            self.cancelScheduled()
            self.upload?.cancel()
            if let id = self.uploadGeneration { self.endUploadTime(id) }
            self.refresh?.cancel()
            self.inboxPush.clearAccount()
        }
        return AppleSyncRuntime(storage: store, nearby: nearby)
    }()
    private var refresh: AppleRefresh?

    func application(_ application: UIApplication,
                     didFinishLaunchingWithOptions launchOptions: [UIApplication.LaunchOptionsKey: Any]? = nil) -> Bool {
        InboxPush.shared.registration = inboxPush
        let center = UNUserNotificationCenter.current()
        center.delegate = self
        center.getNotificationSettings { settings in
            if settings.authorizationStatus == .authorized {
                DispatchQueue.main.async { if self.inboxPush.enabled { application.registerForRemoteNotifications() } }
            }
        }
        let registered = BGTaskScheduler.shared.register(forTaskWithIdentifier: identifier, using: .main) { [self] task in
            guard !reviewingReset else { task.setTaskCompleted(success: true); return }
            schedule()
            var running: AppleRefresh?
            var finished = false
            func finish(_ success: Bool) {
                guard !finished else { return }
                finished = true
                task.expirationHandler = nil
                running = nil
                self.refresh = nil
                task.setTaskCompleted(success: success)
            }
            task.expirationHandler = {
                DispatchQueue.main.async {
                    running?.cancel()
                    finish(false)
                }
            }
            running = runtime.startRefresh { success, shouldRepeat in
                guard !finished else { return }
                if !shouldRepeat.boolValue { self.cancelScheduled() }
                finish(success.boolValue)
            }
            refresh = running
        }
        if !registered { logger.error("Could not register background refresh") }
        return true
    }

    private var uploadTime: UIBackgroundTaskIdentifier = .invalid
    private var upload: AppleRefresh?
    private var uploadGeneration: UUID?

    func enteredBackground() {
        guard !reviewingReset else { return }
        guard runtime.refreshEnabled else { cancelScheduled(); return }
        schedule()
        guard runtime.hasPendingUploads, uploadTime == .invalid else { return }
        let app = UIApplication.shared
        let generation = UUID()
        uploadGeneration = generation
        uploadTime = app.beginBackgroundTask(withName: "Finish queued Yap uploads") { [weak self] in
            guard self?.uploadGeneration == generation else { return }
            self?.upload?.cancel()
            self?.endUploadTime(generation)
        }
        if uploadTime != .invalid {
            upload = runtime.startRefresh { [weak self] _, _ in self?.endUploadTime(generation) }
        }
    }

    private func endUploadTime(_ generation: UUID) {
        guard uploadGeneration == generation, uploadTime != .invalid else { return }
        UIApplication.shared.endBackgroundTask(uploadTime)
        uploadTime = .invalid
        uploadGeneration = nil
        upload = nil
    }

    private func cancelScheduled() {
        BGTaskScheduler.shared.cancel(taskRequestWithIdentifier: identifier)
    }

    private func schedule() {
        let request = BGAppRefreshTaskRequest(identifier: identifier)
        request.earliestBeginDate = Date(timeIntervalSinceNow: 15 * 60)
        do { try BGTaskScheduler.shared.submit(request) }
        catch { logger.info("Background refresh scheduling unavailable: \(error.localizedDescription, privacy: .public)") }
    }
}

@main
struct YappersApp: App {
    @UIApplicationDelegateAdaptor(AppDelegate.self) private var delegate
    @Environment(\.scenePhase) private var scenePhase

    #if DEBUG
    init() {
        // Exit before the scene, Netkit runtime, or background sync can open an account.
        if ProcessInfo.processInfo.arguments.contains("--clear-local-yap-keychain-for-first-run") {
            do {
                try AppleNetkitStorage.clearAccountKeychainForFirstRun()
                FileHandle.standardOutput.write(Data("YAP_FIRST_RUN_LOCAL_KEYCHAIN_VERIFIED_EMPTY\n".utf8))
                exit(EXIT_SUCCESS)
            } catch {
                let failure = error as NSError
                FileHandle.standardError.write(Data("YAP_FIRST_RUN_RESET_FAILED \(failure.domain) \(failure.code)\n".utf8))
                exit(EXIT_FAILURE)
            }
        }
    }
    #endif

    var body: some Scene {
        WindowGroup {
            LaunchView(delegate: delegate)
        }
        .onChange(of: scenePhase) { _, phase in
            if phase == .background { delegate.enteredBackground() }
        }
    }
}

private struct LaunchView: View {
    @ObservedObject var delegate: AppDelegate
    @State private var error: String?

    var body: some View {
        if delegate.reviewingReset {
            ScrollView {
                VStack(alignment: .leading, spacing: 18) {
                    Text("Start over on this iPhone?").font(.largeTitle.bold()).accessibilityAddTraits(.isHeader)
                    Text("Erase your Yap account key, local messages, drafts, and settings from this iPhone. You’ll return to the welcome screen.")
                    Text("To use this account again, you’ll need another signed-in device or a recovery key or file. Published posts and any claimed username stay with the original account.")
                    if let error { Text(error).foregroundStyle(.red) }
                    Button("Erase Yap data on this iPhone", role: .destructive) {
                        do { try delegate.eraseData() }
                        catch { self.error = "The reset couldn’t finish. Unlock this iPhone and try again." }
                    }.buttonStyle(.bordered)
                    if !delegate.resetStarted {
                        Button("Keep my account") { delegate.finishResetReview() }
                    }
                }.frame(maxWidth: 480, alignment: .leading).padding(28)
            }.frame(maxWidth: .infinity)
        } else {
            KotlinView(runtime: delegate.runtime, nearby: delegate.nearby)
                .id(KotlinBuildStamp.value).ignoresSafeArea(.all)
        }
    }
}

private struct KotlinView: UIViewControllerRepresentable {
    let runtime: AppleSyncRuntime
    let nearby: AppleNearbyServices

    func makeUIViewController(context: Context) -> YapTabBarController {
        YapTabBarController(session: AppleAppSession(runtime: runtime, photoLibrary: AppleProfilePhotoLibrary(),
                                                    recoveryFiles: AppleRecoveryFiles(), editorFactory: NativeEditorFactory(), nearby: nearby))
    }

    func updateUIViewController(_ uiViewController: YapTabBarController, context: Context) {}
}
