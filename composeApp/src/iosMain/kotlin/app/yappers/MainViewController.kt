@file:OptIn(kotlinx.cinterop.ExperimentalForeignApi::class)

package app.yappers

import app.yappers.netkit.ffi.yappers_netkit_begin_refresh
import app.yappers.netkit.ffi.yappers_netkit_end_refresh

import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.safeDrawingPadding
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.collectAsState
import androidx.compose.ui.Modifier
import androidx.compose.runtime.CompositionLocalProvider
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.Job
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.cancel
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.collectLatest
import kotlinx.coroutines.CancellationException
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext
import platform.UIKit.UIApplication
import platform.UIKit.UIApplicationState
import platform.UIKit.UIApplicationDidEnterBackgroundNotification
import platform.UIKit.UIApplicationDidBecomeActiveNotification
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.setValue
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.unit.Density
import androidx.compose.ui.window.ComposeUIViewController
import platform.Foundation.NSNotificationCenter
import platform.Foundation.NSOperationQueue
import platform.Foundation.NSUserDefaults
import platform.Foundation.NSProcessInfo
import platform.UIKit.UIContentSizeCategoryDidChangeNotification
import platform.UIKit.UIFont
import platform.UIKit.UIFontTextStyleBody
import platform.UIKit.UIViewController

private class AppleStorage : LocalStorage {
    private val defaults = NSUserDefaults.standardUserDefaults
    private val key = if (NSProcessInfo.processInfo.arguments.contains("--netkit-ui-test")) "yappers.prototype.test.v1" else "yappers.prototype.v1"
    override fun read() = defaults.stringForKey(key)
    override fun write(value: String) { defaults.setObject(value, forKey = key) }
}

private fun preferredFontScale() = (UIFont.preferredFontForTextStyle(UIFontTextStyleBody).pointSize / 17.0).toFloat()

/** One process-wide controller serves UIKit and scheduled launches, even without a scene. */
class AppleSyncRuntime(storage: NetkitStorage, nearby: NearbyServices) {
    private val arguments = NSProcessInfo.processInfo.arguments
    private val walkthroughTest = arguments.contains("--welcome-ui-test")
    internal val controller = NetkitController(storage, NativeNetkitEngine(),
        defaultHome = if (arguments.contains("--netkit-ui-test") && !walkthroughTest) null else ({ nearby.perform("enroll") }))
        .also {
            if (it.offersDefaultHome) it.prepareWelcome(AppleStorage().read() != null)
            it.setForeground(isForeground())
        }

    val refreshEnabled: Boolean get() = controller.state.value.enabled
    val hasPendingUploads: Boolean get() = controller.state.value.let { state ->
        state.pendingCount > 0 || state.messages.any { it.status == "queued" }
    }

    fun startRefresh(completed: (Boolean, Boolean) -> Unit): AppleRefresh {
        val token = yappers_netkit_begin_refresh()
        val job = CoroutineScope(Dispatchers.Main).launch {
            var success = false
            var repeat = true
            try {
                check(token != 0uL)
                val result = withContext(Dispatchers.Default + RefreshScope(token)) {
                    controller.backgroundRefresh(FeedRepository(AppleStorage()).snapshot.nearby)
                }
                success = result != false
                repeat = result != null
            } catch (cancelled: CancellationException) {
                throw cancelled
            } catch (_: Exception) {
                // A locked or unavailable store can be retried on the next OS opportunity.
            } finally {
                yappers_netkit_end_refresh(token)
                completed(success, repeat)
            }
        }
        return AppleRefresh(token, job)
    }
}

class AppleRefresh internal constructor(private val token: ULong, private val job: Job) {
    fun cancel() {
        yappers_netkit_end_refresh(token)
        job.cancel()
    }
}

private fun isForeground() = UIApplication.sharedApplication.applicationState == UIApplicationState.UIApplicationStateActive

class ApplePostRoute internal constructor(post: Post, internal val destination: PostDestination) {
    val title = "Post by " + if (isAnonymous(post.author, post.visibility)) "Anonymous neighbor"
        else post.username?.takeUnless { it.isBlank() } ?: post.author.ifBlank { "Neighbor" }
}

class AppleToolbarActions internal constructor(
    val postEnabled: Boolean,
    val refreshLabel: String?,
    val inboxCount: Int,
    private val onPost: () -> Unit,
    private val onRefresh: () -> Unit,
) {
    fun post() = onPost()
    fun refresh() = onRefresh()
}

/** Native controllers share one repository, onboarding state, and foreground sync job. */
class AppleAppSession(
    private val runtime: AppleSyncRuntime,
    private val photoLibrary: ProfilePhotoLibrary,
    private val recoveryFiles: RecoveryFiles,
    private val editorFactory: AppleEditorFactory,
    private val nearby: NearbyServices,
) {
    private val storage = AppleStorage()
    private val content = AppContentState(storage)
    private val networking = runtime.controller
    private val scope = CoroutineScope(SupervisorJob() + Dispatchers.Main)
    private val center = NSNotificationCenter.defaultCenter
    private val defaults = NSUserDefaults.standardUserDefaults
    private val arguments = NSProcessInfo.processInfo.arguments
    private val installKey = if (arguments.contains("--netkit-ui-test")) "yap.welcome.test.complete" else "yap.welcome.complete"
    private var installWelcome by mutableStateOf(networking.offersDefaultHome && !defaults.boolForKey(installKey))
    val initialTab = if (arguments.contains("--show-groups")) "Groups" else "Nearby"
    private var activeTab by mutableStateOf(initialTab)
    private val foreground = MutableStateFlow(isForeground())
    private var fontScale by mutableStateOf(preferredFontScale())
    private val observers = listOf(
        center.addObserverForName(UIApplicationDidEnterBackgroundNotification, null, NSOperationQueue.mainQueue) {
            foreground.value = false
            networking.setForeground(false)
        },
        center.addObserverForName(UIApplicationDidBecomeActiveNotification, null, NSOperationQueue.mainQueue) {
            networking.setForeground(true)
            foreground.value = true
        },
        center.addObserverForName(UIContentSizeCategoryDidChangeNotification, null, NSOperationQueue.mainQueue) {
            fontScale = preferredFontScale()
        }
    )

    init {
        networking.setForeground(foreground.value)
        scope.launch {
            foreground.collectLatest { active ->
                if (active) networking.pollWhileOpen({ content.repository.snapshot })
                else networking.shutdown()
            }
        }
    }

    val tabsVisible: Boolean get() = !installWelcome && !networking.welcome.value && !networking.backupStep.value

    fun didSelectTab(tab: String) { activeTab = tab }

    fun close() {
        observers.forEach { center.removeObserver(it) }
        scope.cancel()
    }

    fun makeTab(
        tab: String,
        onSelectTab: (String) -> Unit,
        onSetRootBars: (Boolean, Boolean) -> Unit,
        onToolbar: (AppleToolbarActions) -> Unit,
        onOpenProfileSettings: () -> Unit,
        onOpenPrivateGroups: () -> Unit,
        onOpenInbox: (InboxPage, MessageContact?) -> Unit,
        onOpenPost: (ApplePostRoute) -> Unit,
    ): UIViewController = makeContent(tab, PlatformNavigation(
        activeTab = { activeTab },
        selectTab = onSelectTab,
        setRootBars = onSetRootBars,
        updateToolbar = { enabled, label, count, post, refresh -> onToolbar(AppleToolbarActions(enabled, label, count, post, refresh)) },
        openInbox = onOpenInbox,
        openProfileSettings = onOpenProfileSettings,
        openPrivateGroups = onOpenPrivateGroups,
        openPost = { post, destination -> onOpenPost(ApplePostRoute(post, destination)) },
        closePost = {},
    ))

    fun makePost(tab: String, route: ApplePostRoute, onOpenInbox: (InboxPage, MessageContact?) -> Unit, onClose: () -> Unit): UIViewController =
        makeContent(tab, PlatformNavigation(
            activeTab = { activeTab },
            selectTab = {},
            openInbox = onOpenInbox,
            openPost = { _, _ -> },
            closePost = onClose,
            post = route.destination,
        ))

    fun makeProfileSettings(onSelectTab: (String) -> Unit, onOpenGuide: () -> Unit,
                            onOpenInbox: (InboxPage, MessageContact?) -> Unit): UIViewController =
        makeContent("You", PlatformNavigation(
            activeTab = { activeTab }, selectTab = onSelectTab, openInbox = onOpenInbox,
            openGuide = onOpenGuide, profileSettings = true, openPost = { _, _ -> }, closePost = {},
        ))

    fun makeGuide(onClose: () -> Unit, onDone: () -> Unit): UIViewController = makeContent("You", PlatformNavigation(
        activeTab = { activeTab }, selectTab = {}, openInbox = { _, _ -> }, openPost = { _, _ -> },
        closePost = {}, dismissGuide = onClose, finishGuide = onDone,
    ))

    fun makeInbox(tab: String, page: InboxPage, contact: MessageContact?, onSelectTab: (String) -> Unit,
                  onOpenInbox: (InboxPage, MessageContact?) -> Unit, onOpenProfileSettings: () -> Unit,
                  onOpenPost: (ApplePostRoute) -> Unit, onCloseStack: () -> Unit): UIViewController {
        val live = networking.state.value.enabled
        val scope = networking.state.value.messagingScope
        return makeContent(tab, PlatformNavigation(
            activeTab = { activeTab },
            selectTab = onSelectTab,
            openInbox = onOpenInbox,
            openPost = { post, destination -> onOpenPost(ApplePostRoute(post, destination)) },
            closePost = {},
            inboxPage = page,
            openProfileSettings = onOpenProfileSettings,
            matchesInboxScope = { it.enabled == live && it.messagingScope == scope },
            closeInboxStack = onCloseStack,
            contact = contact,
        ))
    }

    fun makePrivateGroups(group: String?, onOpenGroup: (String, String) -> Unit, onClose: () -> Unit): UIViewController {
        val account = networking.state.value.messagingScope
        return host {
            val state by networking.state.collectAsState()
            if (state.enabled && state.ownershipReady && state.messagingScope == account) {
                YapTheme {
                    Box(Modifier.fillMaxSize().safeDrawingPadding()) {
                        PrivateGroupsScreen(networking, state.snapshot.alias, onBack = onClose,
                            routeGroup = group, pushGroup = onOpenGroup)
                    }
                }
            } else LaunchedEffect(Unit) { onClose() }
        }
    }

    private fun makeContent(tab: String, navigation: PlatformNavigation): UIViewController = host {
        YappersApp(storage, networking, initialTab = tab, photoLibrary = photoLibrary, recoveryFiles = recoveryFiles,
            nearbyServices = nearby, syncInView = false, installationWelcome = installWelcome, onWelcomeComplete = {
                defaults.setBool(true, forKey = installKey)
                installWelcome = false
            }, contentState = content, platformNavigation = navigation)
    }

    private fun host(screen: @Composable () -> Unit): UIViewController = ComposeUIViewController {
        val density = LocalDensity.current
        val host = androidx.compose.ui.uikit.LocalUIViewController.current
        CompositionLocalProvider(
            LocalRefreshIndicator provides { progress, refreshing -> AppleRefreshIndicatorView(editorFactory, progress, refreshing) },
            LocalTransientNotice provides { message -> editorFactory.showTransientNotice(host, message) },
            LocalSkiaAppearance provides { content -> AppleAppearance(content) },
            LocalNativeForm provides { form, mode, modifier -> AppleFormHost(editorFactory, form, mode, modifier) },
            LocalNativeConfirmation provides { confirmation -> AppleConfirmation(editorFactory, confirmation) },
            LocalDensity provides Density(density.density, fontScale),
            LocalRepliesButton provides { count, open -> AppleRepliesButton(count, open) },
            LocalNativeCrop provides { model -> ApplePhotoCropSheet(editorFactory, model) },
            LocalChatBubble provides { bubble, modifier -> AppleChatBubble(editorFactory, bubble, modifier) },
            LocalFeedControl provides { row, modifier -> AppleFeedButton(editorFactory, row, modifier) },
            LocalInlineEditor provides { model -> AppleInlineEditorView(editorFactory, model) },
            LocalPostSheetPresenter provides { model -> ApplePostSheet(model, editorFactory) },
        ) {
            screen()
        }
    }
}
