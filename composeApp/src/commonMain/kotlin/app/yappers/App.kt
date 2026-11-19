@file:OptIn(androidx.compose.material3.ExperimentalMaterial3Api::class)

package app.yappers

import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.LazyListState
import androidx.compose.foundation.lazy.rememberLazyListState
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.selection.selectable
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.material3.*
import androidx.compose.ui.semantics.CustomAccessibilityAction
import androidx.compose.ui.semantics.customActions
import androidx.compose.ui.platform.testTag
import androidx.compose.runtime.*
import androidx.compose.runtime.retain.retain
import androidx.compose.runtime.saveable.rememberSaveable
import kotlinx.serialization.json.JsonObject
import kotlinx.serialization.json.jsonPrimitive
import kotlinx.coroutines.CancellationException
import kotlinx.coroutines.launch
import androidx.compose.ui.text.input.PasswordVisualTransformation
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalFocusManager
import androidx.compose.ui.platform.LocalSoftwareKeyboardController
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.focus.FocusRequester
import androidx.compose.ui.focus.focusRequester
import androidx.compose.ui.focus.onFocusChanged
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.semantics.clearAndSetSemantics
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.semantics.LiveRegionMode
import androidx.compose.ui.semantics.liveRegion
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextDecoration
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp

internal val LightColors = lightColorScheme(
    primary = Color(0xFF694394), onPrimary = Color.White,
    primaryContainer = Color(0xFFE8D9FF), onPrimaryContainer = Color(0xFF2D1749),
    secondary = Color(0xFF694394), onSecondary = Color.White,
    secondaryContainer = Color(0xFFE8D9FF), onSecondaryContainer = Color(0xFF2D1749),
    surfaceContainerLow = Color(0xFFFCF8FF), surfaceContainerHigh = Color(0xFFF0E7F8),
    background = Color(0xFFFCF8FF), onBackground = Color(0xFF2A2034),
    surface = Color(0xFFFCF8FF), onSurface = Color(0xFF2A2034),
    surfaceVariant = Color(0xFFF0E7F8), onSurfaceVariant = Color(0xFF695B76),
    outlineVariant = Color(0xFFDDD0E9)
)
internal val DarkColors = darkColorScheme(
    primary = Color(0xFFD7B5FF), onPrimary = Color(0xFF352048),
    primaryContainer = Color(0xFF503568), onPrimaryContainer = Color(0xFFE8D9FF),
    secondary = Color(0xFFD7B5FF), onSecondary = Color(0xFF352048),
    secondaryContainer = Color(0xFF503568), onSecondaryContainer = Color(0xFFE8D9FF),
    surfaceContainerLow = Color(0xFF1B1522), surfaceContainerHigh = Color(0xFF302639),
    background = Color(0xFF1B1522), onBackground = Color(0xFFF6ECFF),
    surface = Color(0xFF1B1522), onSurface = Color(0xFFF6ECFF),
    surfaceVariant = Color(0xFF302639), onSurfaceVariant = Color(0xFFCEBDD9),
    outlineVariant = Color(0xFF50425D)
)

internal class AppContentState(storage: LocalStorage) {
    val repository = FeedRepository(storage)
    val localProfile = mutableStateOf(repository.snapshot)
    var feedReset by mutableIntStateOf(0)
}

internal class PostDestination(val id: Long, val atComments: Boolean, val group: String?, val live: Boolean)

internal class PlatformNavigation(
    val activeTab: () -> String,
    val selectTab: (String) -> Unit,
    val setRootBars: (tabs: Boolean, toolbar: Boolean) -> Unit = { _, _ -> },
    val updateToolbar: ((Boolean, String?, () -> Unit, () -> Unit) -> Unit)? = null,
    val openInbox: (InboxPage, MessageContact?) -> Unit,
    val inboxPage: InboxPage? = null,
    val openGuide: (() -> Unit)? = null,
    val openProfileSettings: (() -> Unit)? = null,
    val profileSettings: Boolean = false,
    val openPrivateGroups: (() -> Unit)? = null,
    val dismissGuide: (() -> Unit)? = null,
    val finishGuide: (() -> Unit)? = null,
    val matchesInboxScope: (NetkitState) -> Boolean = { true },
    val closeInboxStack: () -> Unit = {},
    val contact: MessageContact? = null,
    val openPost: (Post, PostDestination) -> Unit,
    val closePost: () -> Unit,
    val post: PostDestination? = null,
)

@Composable
internal fun YappersApp(storage: LocalStorage, networking: NetkitController? = null, foreground: Boolean = true, initialTab: String = "Nearby", photoLibrary: ProfilePhotoLibrary? = null, desktop: Boolean = false, recoveryFiles: RecoveryFiles? = null, nearbyServices: NearbyServices? = null, syncInView: Boolean = true, installationWelcome: Boolean = false, onWelcomeComplete: () -> Unit = {}, contentState: AppContentState? = null, platformNavigation: PlatformNavigation? = null) {
    val session = contentState ?: remember(storage) { AppContentState(storage) }
    val repository = session.repository
    var demoState by session.localProfile
    val connection by (networking?.state ?: remember { kotlinx.coroutines.flow.MutableStateFlow(NetkitState()) }).collectAsState()
    val live = connection.enabled && networking != null
    val editorUsernameClaim = rememberEditorUsernameClaim(connection, networking)
    val state = if (live || connection.awaitingDefault) connection.snapshot else demoState.privacy.filter(demoState.withDemoBlocks())
    val identityScope = if (live) connection.messagingScope else "demo"
    val anonymous = identityScope !in demoState.namedPostingScopes
    val namedIdentity = if (live) connection.privacy.username ?: (if (connection.needsUsername) "my username" else state.alias) else state.alias
    val nearby = demoState.nearby
    var filtersOpen by remember { mutableStateOf(false) }
    val scope = rememberCoroutineScope()
    LaunchedEffect(networking, foreground, syncInView) {
        if (networking == null || !syncInView) return@LaunchedEffect
        if (foreground) networking.pollWhileOpen({ repository.snapshot })
        else networking.shutdown()
    }
    var revisitGuide by remember { mutableStateOf(false) }
    var tab by rememberSaveable(initialTab) { mutableStateOf(initialTab) }
    val materialNavigation = LocalMaterialNavigation.current
    var privateGroupsOpen by retain(identityScope) { mutableStateOf(false) }
    val activeTab = platformNavigation?.activeTab?.invoke() ?: tab
    val nativeForms = LocalNativeForm.current != null
    val feedButton = LocalFeedControl.current
    val nativePost = platformNavigation?.post
    val inboxPage = platformNavigation?.inboxPage
    LaunchedEffect(tab, live, activeTab, foreground, privateGroupsOpen) {
        if (foreground && !privateGroupsOpen && tab == "Groups" && tab == activeTab && live &&
            nativePost == null && inboxPage == null) networking.refreshGroups()
    }
    var reportTarget by remember(connection.messagingScope, live) { mutableStateOf<ReportTarget?>(null) }
    val openReport: ((ReportTarget) -> Unit)? = if (live) ({ reportTarget = it }) else null
    var privacyOpen by retain(connection.messagingScope, live) { mutableStateOf(false) }
    val policy = if (live) connection.privacy else demoState.privacy
    var inbox by retain(connection.messagingScope, live) { mutableStateOf(inboxPage != null) }
    var mailSection by retain(connection.messagingScope, live) { mutableStateOf(InboxPage.Messages) }
    var dmContact by retain(connection.messagingScope, live) { mutableStateOf(platformNavigation?.contact) }
    var blocking by remember(connection.messagingScope, live) { mutableStateOf<BlockChoice?>(null) }
    var topic by remember { mutableStateOf("All") }
    var popular by remember { mutableStateOf(false) }
    val nearbyScroll = key(live, connection.activeGroup?.id, nearby, topic, popular) { rememberLazyListState() }
    val savedScroll = key(live, connection.activeGroup?.id) { rememberLazyListState() }
    val profileScroll = rememberLazyListState()
    LaunchedEffect(session.feedReset) {
        if (platformNavigation != null && tab == "Nearby" && nativePost == null && session.feedReset > 0) {
            topic = "All"
            popular = false
            nearbyScroll.requestScrollToItem(0)
        }
    }
    var editingProfile by remember { mutableStateOf(false) }
    var postDraft by retain(identityScope, connection.activeGroup?.id) { mutableStateOf<PostDraft?>(null) }
    var shownAuthor by remember(connection.messagingScope) { mutableStateOf<SharedAuthor?>(null) }
    var selectedId by retain(identityScope, connection.activeGroup?.id) { mutableStateOf<Long?>(null) }
    var openAtComments by retain(identityScope, connection.activeGroup?.id) { mutableStateOf(false) }
    fun openConversation(id: Long, atComments: Boolean = false) {
        if (platformNavigation != null) {
            val post = state.posts.firstOrNull { it.id == id && !it.hidden } ?: return
            platformNavigation.openPost(post, PostDestination(id, atComments, connection.activeGroup?.id, live))
        } else {
            openAtComments = atComments
            selectedId = id
        }
    }
    fun closeConversation() {
        if (nativePost != null) platformNavigation?.closePost?.invoke()
        else selectedId = null
    }
    var deleteTarget by remember { mutableStateOf<String?>(null) }
    var hidePostId by remember { mutableStateOf<Long?>(null) }
    var locationInfo by remember { mutableStateOf(false) }
    var resetConfirm by remember { mutableStateOf(false) }
    var error by remember { mutableStateOf<String?>(null) }
    val snackbars = remember { SnackbarHostState() }
    var notice by remember { mutableStateOf<String?>(null) }
    val nativeNotice = LocalTransientNotice.current
    fun inform(message: String) {
        if (nativeNotice != null) nativeNotice(message) else notice = message
    }
    fun change(action: () -> Unit): Boolean {
        return try { action(); demoState = repository.snapshot; true } catch (failure: Exception) {
            error = failure.message ?: "Couldn't save your change. Please try again."
            false
        }
    }
    fun chooseIdentity(asAnonymous: Boolean) {
        identityScope?.let { change { repository.setPostingIdentity(it, asAnonymous) } }
    }
    fun act(action: suspend () -> Unit) {
        scope.launch { try { action() } catch (failure: Exception) {
            if (failure is CancellationException) throw failure
            error = failure.message ?: "Please try again."
        } }
    }
    fun savePrivacy(update: (PrivacySettings) -> PrivacySettings) {
        if (live) act { networking.setPrivacy(update); scope.launch { networking.sync() } }
        else change { repository.setPrivacy(repository.snapshot.privacy.let { it.copy(settings = update(it.settings)) }) }
    }
    fun mute(id: String, title: String?) {
        if (live) act { networking.muteThread(id, title) }
        else change {
            val current = repository.snapshot.privacy
            repository.setPrivacy(current.copy(mutedThreads = if (title == null) current.mutedThreads - id else current.mutedThreads + (id to title)))
        }
        if (title != null) inform("Thread muted. Manage muted threads in Privacy.")
        closeConversation()
    }
    fun mutePost(post: Post) = mute(post.remoteId ?: "demo-${post.id}", post.headline.ifBlank { post.body.take(80) })
    fun contact(event: String) = act {
        val contact = checkNotNull(networking).messageContact(event)
        shownAuthor = null
        if (platformNavigation != null) {
            platformNavigation.openInbox(InboxPage.Conversation, contact)
        } else {
            dmContact = contact
            inbox = true
        }
    }
    fun showAuthor(post: Post) {
        shownAuthor = SharedAuthor(post.author, post.username, post.profile, post.remoteId, "post-${post.id}", post.mine, post.visibility?.peer)
    }
    fun showAuthor(reply: Reply) {
        shownAuthor = SharedAuthor(reply.author, reply.username, null, reply.remoteId, "reply-${reply.id}", reply.mine, reply.visibility?.peer)
    }
    fun navigate(destination: String) {
        privateGroupsOpen = false
        privacyOpen = false
        inbox = false
        mailSection = InboxPage.Messages
        dmContact = null
        selectedId = null
        if (platformNavigation == null) tab = destination
        else platformNavigation.selectTab(destination)
    }
    fun openCommunitySettings() {
        navigate("You")
        platformNavigation?.openProfileSettings?.invoke()
    }
    fun returnToNearby() {
        topic = "All"
        popular = false
        nearbyScroll.requestScrollToItem(0)
        if (platformNavigation != null) session.feedReset++
        navigate("Nearby")
    }
    fun vote(post: Post, value: Int) {
        if (live) act { networking.vote(post.id, value); networking.sync() }
        else change { repository.vote(post.id, value) }
    }
    fun save(post: Post) {
        if (live) act { networking.save(post.id) } else change { repository.save(post.id) }
    }
    suspend fun saveProfile(name: String, profile: CommunityProfile) {
        if (networking?.state?.value?.enabled == true) networking.setAlias(name)
        repository.setProfile(name, profile)
        demoState = repository.snapshot
    }
    fun goBack() {
        when {
            revisitGuide -> revisitGuide = false
            editingProfile -> editingProfile = false
            privacyOpen -> privacyOpen = false
            dmContact != null -> dmContact = null
            inbox && mailSection != InboxPage.Messages -> mailSection = InboxPage.Messages
            inbox -> inbox = false
            selectedId != null -> selectedId = null
            privateGroupsOpen -> privateGroupsOpen = false
            else -> navigate("Nearby")
        }
    }
    val materialStack = if (materialNavigation == null) emptyList() else buildList {
        add(MaterialPage.Home)
        if (privateGroupsOpen && live && tab == "Groups") add(MaterialPage.PrivateGroups)
        selectedId?.takeIf { id -> state.posts.any { it.id == id && !it.hidden } }?.let {
            add(MaterialPage.Thread(it, openAtComments))
        }
        if (inbox) {
            add(MaterialPage.Mail(InboxPage.Messages))
            if (mailSection != InboxPage.Messages) add(MaterialPage.Mail(mailSection))
            dmContact?.let { add(MaterialPage.Mail(InboxPage.Conversation, it)) }
        }
        if (privacyOpen) add(MaterialPage.Privacy)
        if (editingProfile) add(MaterialPage.EditProfile)
        if (revisitGuide) add(MaterialPage.Guide)
    }
    // NavDisplay owns detail Back events, including interactive gesture progress.
    val handleSharedBack = if (materialNavigation == null)
        dmContact != null || inbox || privacyOpen || selectedId != null || tab != "Nearby"
    else materialStack.size == 1 && tab != "Nearby"
    AppBackHandler(!editingProfile && handleSharedBack, ::goBack)
    AppBackHandler(revisitGuide && materialNavigation == null) { revisitGuide = false }
    val firstVisit = networking?.welcome?.collectAsState()?.value == true
    val offerBackup = networking?.backupStep?.collectAsState()?.value == true
    val usernameStatus = networking?.welcomeUsername?.collectAsState()?.value ?: WelcomeUsername.Unchecked
    val verifyUsername = offerBackup || installationWelcome && networking?.offersDefaultHome == true &&
        (connection.awaitingDefault || connection.homeLabel == AppBrand.domain)
    val welcoming = networking != null && (firstVisit || offerBackup || revisitGuide || installationWelcome)
    if (platformNavigation != null && inboxPage != null && (welcoming || !platformNavigation.matchesInboxScope(connection))) {
        LaunchedEffect(Unit) { platformNavigation.closeInboxStack() }
        return
    }
    if (platformNavigation != null && nativePost == null && inboxPage == null && platformNavigation.finishGuide == null) {
        LaunchedEffect(welcoming, editingProfile, privacyOpen) {
            val showBars = !welcoming && (!editingProfile || nativeForms)
            platformNavigation.setRootBars(showBars, showBars && !privacyOpen)
        }
    }
    YapTheme {
        val colors = MaterialTheme.colorScheme
        if (networking != null && platformNavigation?.finishGuide != null) {
            WelcomeBackupScreen(networking, connection.ownershipReady && live, replay = true,
                vault = recoveryFiles as? RecoveryKeyVault, onClose = platformNavigation.dismissGuide, onDone = platformNavigation.finishGuide)
            return@YapTheme
        }
        if (networking != null && welcoming && !(materialNavigation != null && revisitGuide) && nativePost == null && inboxPage == null) {
                var signup by remember { mutableStateOf<WelcomeProfile?>(null) }
                var attempt by remember { mutableIntStateOf(0) }
                var enrollmentError by remember { mutableStateOf<String?>(null) }
                LaunchedEffect(signup, attempt) {
                    val draft = signup ?: return@LaunchedEffect
                    enrollmentError = null
                    try { networking.finishWelcome(draft.name, profile = draft.profile, namedPosting = draft.namedPosting) }
                    catch (failure: Exception) {
                        if (failure is CancellationException) throw failure
                        enrollmentError = failure.message ?: "Couldn’t join your home. Try again."
                    }
                }
                if (firstVisit && signup == null) WelcomeScreen(networking, recoveryFiles, photoLibrary,
                    onCreateProfile = if (platformNavigation != null) ({ signup = it }) else null)
                else if ((firstVisit || verifyUsername) && usernameStatus != WelcomeUsername.Confirmed) {
                    WelcomeUsernameScreen(networking, connection, usernameStatus, creatingProfile = signup != null,
                        enrolling = firstVisit, enrollmentError = enrollmentError, retryEnrollment = { attempt++ }) { draft, postingScope ->
                        val namedScope = if (draft.namedPosting) checkNotNull(postingScope) else null
                        repository.setProfile(draft.name, draft.profile, namedScope)
                        demoState = repository.snapshot
                    }
                } else if (offerBackup && networking.finishingRestore) {
                    LaunchedEffect(networking) {
                        onWelcomeComplete()
                        networking.enterNeighborhood()
                    }
                } else WelcomeBackupScreen(networking, connection.ownershipReady && live, replay = revisitGuide, vault = recoveryFiles as? RecoveryKeyVault,
                    returningName = if (installationWelcome && !offerBackup) connection.snapshot.alias else null,
                    onSaveName = { alias ->
                        check(live) { "Reconnect to your home to change your name, or continue with your current name." }
                        networking.setAlias(alias)
                        repository.setAlias(alias)
                        demoState = repository.snapshot
                    }, onDone = { onWelcomeComplete(); revisitGuide = false; navigate("Nearby") })
                return@YapTheme
        }
        @Composable
        fun Screen(page: MaterialPage?) {
            if (page == MaterialPage.PrivateGroups && networking != null) {
                Surface(Modifier.fillMaxSize(), color = colors.background) {
                    Box(Modifier.fillMaxSize().safeDrawingPadding()) {
                        PrivateGroupsScreen(networking, connection.snapshot.alias, onBack = ::goBack)
                    }
                }
                return
            }
            if (page == MaterialPage.Guide && networking != null) {
                WelcomeBackupScreen(networking, connection.ownershipReady && live, replay = true,
                    vault = recoveryFiles as? RecoveryKeyVault, onDone = { revisitGuide = false; navigate("Nearby") })
                return
            }
            val mail = page as? MaterialPage.Mail
            val showInbox = if (page == null) inbox else mail != null
            val showPrivacy = if (page == null) privacyOpen || inboxPage == InboxPage.Privacy else page == MaterialPage.Privacy
            val currentContact = if (page == null) dmContact else mail?.contact
            val thread = page as? MaterialPage.Thread
            val threadId = if (page == null) nativePost?.id ?: selectedId else thread?.id
            val selected = state.posts.firstOrNull { it.id == threadId && !it.hidden &&
                (nativePost == null || (nativePost.group == connection.activeGroup?.id && nativePost.live == live)) }
            @Composable
            fun ThreadContent(post: Post, dedicated: Boolean) {
                Conversation(post, state.alias, namedIdentity, anonymous, ::chooseIdentity, live, backLabel = tab,
                    openAtComments = thread?.atComments ?: nativePost?.atComments ?: openAtComments, onBack = ::closeConversation,
                    needsUsername = live && connection.needsUsername, usernameControl = { UsernameControl(connection, networking) },
                    editorUsernameClaim = editorUsernameClaim,
                    onReply = { body, anonymous ->
                        if (!live) change { repository.reply(post.id, body, anonymous) }
                        else try { networking.reply(post.id, body, anonymous); scope.launch { networking.sync() }; true }
                        catch (failure: Exception) { if (failure is CancellationException) throw failure; error = failure.message; false }
                    },
                    onVote = { direction -> vote(post, direction) },
                    onReplyActions = { reply ->
                        val event = reply.remoteId
                        if (live && event != null) blocking = BlockChoice(event, reply.author, true, reply.visibility?.peer)
                        else if (!live) blocking = BlockChoice("reply-${reply.id}", reply.author, true)
                    },
                    replyActionsEnabled = !live || connection.messagingScope != null,
                    onReport = openReport, onMute = { mutePost(post) }, onSave = { save(post) }, onHide = { hidePostId = post.id },
                    onDelete = { deleteTarget = it }, onAuthor = { showAuthor(post) },
                    onReplyAuthor = { showAuthor(it) }, dedicated = dedicated,
                    refreshReplies = if (dedicated && live) ({
                        try {
                            networking.sync()
                            if (networking.state.value.feedUnavailable) inform("Couldn’t reach your home. Showing saved replies.")
                        } catch (failure: Exception) {
                            if (failure is CancellationException) throw failure
                            inform("Couldn’t refresh replies. Try again.")
                        }
                    }) else null)
            }
            if (nativePost != null) {
                Surface(Modifier.fillMaxSize(), color = colors.background) {
                    Box(Modifier.fillMaxSize().safeDrawingPadding()) {
                        if (selected != null) ThreadContent(selected, dedicated = true)
                        else Text("This post is no longer available.", Modifier.padding(22.dp))
                    }
                }
            } else if (page == MaterialPage.EditProfile || (page == null && editingProfile && !nativeForms)) {
                Surface(Modifier.fillMaxSize(), color = colors.background) {
                    Box(Modifier.fillMaxSize().safeDrawingPadding(), contentAlignment = Alignment.TopCenter) {
                        ProfileEditor(state.alias, demoState.profile, photoLibrary,
                            onDismiss = { editingProfile = false }, onSave = ::saveProfile)
                    }
                }
            } else BoxWithConstraints(Modifier.fillMaxSize()) {
                val expanded = desktop && maxWidth >= 900.dp
                val showingThread = selected?.takeUnless { showInbox || showPrivacy }
                val sideBySide = showingThread != null && desktop && maxWidth >= 1200.dp
                val detail = showingThread != null || showInbox || showPrivacy
                AppNavigationLayout(tab, ::navigate, desktopSidebar = expanded, showDestinations = !detail) { visibleTab ->
                    val feedFab = materialNavigation != null && !detail && (visibleTab == "Nearby" || visibleTab == "Saved")
                    AppFrame(nativeNavigation = platformNavigation != null,
                        floatingActionButton = {
                            if (feedFab && !connection.awaitingDefault) {
                                val scroll = if (visibleTab == "Saved") savedScroll else nearbyScroll
                                val expandedFab by remember(scroll) { derivedStateOf { !scroll.canScrollBackward } }
                                ExtendedFloatingActionButton(
                                    onClick = { postDraft = PostDraft() },
                                    modifier = Modifier.semantics { contentDescription = "Post" },
                                    expanded = expandedFab,
                                    icon = { LineIcon("edit", null, tint = LocalContentColor.current) },
                                    text = { Text("Post") },
                                    containerColor = colors.primary,
                                    contentColor = colors.onPrimary
                                )
                            }
                        },
                        snackbarHost = { if (materialNavigation != null) SnackbarHost(snackbars) },
                        bottomBar = {
                            if (!expanded && platformNavigation == null && materialNavigation == null) YappersBottomNavigation(tab) { destination ->
                                navigate(destination)
                            }
                        }
                    ) { padding ->
                        // Descendant IME padding must exclude the bars already accounted for by Scaffold.
                        Box(Modifier.fillMaxSize().padding(padding).consumeWindowInsets(padding), contentAlignment = Alignment.TopCenter) {
                            Column(Modifier.widthIn(max = if (sideBySide) 1160.dp else 760.dp).fillMaxSize()) {
                                val groupsVisible = visibleTab == "Groups" && !showInbox && !showPrivacy && showingThread == null
                                val refreshLabel = when {
                                    showPrivacy -> "Refresh privacy settings"
                                    showInbox && currentContact != null -> "Refresh conversation"
                                    showInbox -> "Refresh inbox"
                                    showingThread != null -> "Refresh replies"
                                    groupsVisible -> "Refresh groups"
                                    visibleTab == "You" -> "Refresh profile"
                                    else -> "Refresh posts"
                                }
                                val canRefresh = networking != null && (live || connection.awaitingDefault)
                                var refreshing by remember { mutableStateOf(false) }
                                fun refreshContent() {
                                    if (!canRefresh || refreshing) return
                                    refreshing = true
                                    act {
                                        try {
                                            if (!networking.state.value.enabled) networking.restore(repository.snapshot.alias)
                                            if (groupsVisible && networking.state.value.enabled) networking.refreshGroups()
                                            else {
                                                networking.sync()
                                                if (inbox && (networking.state.value.directAvailable || networking.state.value.mailboxElsewhere)) networking.refreshBlocks()
                                            }
                                        } finally { refreshing = false }
                                    }
                                }
                                val updateToolbar = platformNavigation?.updateToolbar
                                if (updateToolbar != null) {
                                    val refresh by rememberUpdatedState { refreshContent() }
                                    val accessibleRefresh = refreshLabel.takeIf { canRefresh && !refreshing }
                                    LaunchedEffect(updateToolbar, connection.awaitingDefault, accessibleRefresh) {
                                        updateToolbar(!connection.awaitingDefault, accessibleRefresh, { postDraft = PostDraft() }, { refresh() })
                                    }
                                }
                                if (materialNavigation != null && !detail) {
                                    CenterAlignedTopAppBar(
                                        title = { YapWordmark(Modifier.semantics {
                                            if (canRefresh && !refreshing) customActions = listOf(
                                                CustomAccessibilityAction(refreshLabel) { refreshContent(); true }
                                            )
                                        }) },
                                        navigationIcon = {
                                            FilledTonalIconButton(onClick = { inbox = true; dmContact = null }, modifier = Modifier.padding(start = 8.dp)) {
                                                LineIcon("inbox", "Inbox", tint = LocalContentColor.current)
                                            }
                                        },
                                        windowInsets = WindowInsets(0)
                                    )
                                } else if (materialNavigation == null && platformNavigation == null) Row(
                                    Modifier.fillMaxWidth().heightIn(min = 56.dp),
                                    verticalAlignment = Alignment.CenterVertically
                                ) {
                                    Box(Modifier.weight(1f).padding(start = 12.dp)) {
                                        Row(verticalAlignment = Alignment.CenterVertically) {
                                            if (!showInbox && !showPrivacy) TextButton(onClick = { inbox = true; dmContact = null }) { Text("Inbox") }
                                            if (desktop && canRefresh) IconButton(enabled = !refreshing, onClick = ::refreshContent) {
                                                LineIcon("refresh", refreshLabel)
                                            }
                                        }
                                    }
                                    YapWordmark(Modifier.semantics {
                                        if (canRefresh && !refreshing) customActions = listOf(
                                            CustomAccessibilityAction(refreshLabel) { refreshContent(); true }
                                        )
                                    })
                                    Box(Modifier.weight(1f).padding(end = 14.dp), contentAlignment = Alignment.CenterEnd) {
                                        if (!showPrivacy && !showInbox && (showingThread == null || sideBySide) && visibleTab != "Groups") {
                                            FilledTonalButton(
                                                enabled = !connection.awaitingDefault,
                                                onClick = { postDraft = PostDraft() },
                                                contentPadding = PaddingValues(horizontal = 16.dp)
                                            ) {
                                                LineIcon("plus", null, Modifier.size(18.dp))
                                                Spacer(Modifier.width(6.dp))
                                                Text("Post")
                                            }
                                        }
                                    }
                                }
                                Row(Modifier.weight(1f).fillMaxWidth()) {
                                    if (showingThread == null || sideBySide) FeedRefreshPane(
                                        Modifier.weight(1f).fillMaxHeight(),
                                        enabled = canRefresh && !(nativeForms && groupsVisible),
                                        refreshing = refreshing, onRefresh = ::refreshContent,
                                    ) {
                                        when {
                                            showPrivacy -> PrivacyScreen(policy, when {
                                                !live -> "Settings for this local demo."
                                                !connection.supportsPrivacy && connection.online -> "Your device applies these filters. This home uses an older version."
                                                policy.hasPending -> "Changes saved on this device. Waiting for your home to sync."
                                                else -> null
                                            }, onReport = openReport, onClose = { privacyOpen = false }, onSave = ::savePrivacy, onUnmute = { mute(it, null) }, nativeTitle = inboxPage == InboxPage.Privacy)
                                            showInbox -> InboxScreen(connection, currentContact,
                                                if (live) (policy.blocks.map { BlockedAccount(it.handle ?: "local-" + it.value, it.label) } +
                                                    connection.blockedAccounts.filterNot { it.id in policy.pendingUnblocks }).distinctBy { it.id } else demoState.demoBlocks.map { BlockedAccount(it.key, it.value) },
                                                onPrivacy = { privacyOpen = true }, onReport = openReport, nativePage = mail?.section ?: inboxPage,
                                                openPage = { section ->
                                                    if (materialNavigation != null) mailSection = section
                                                    else platformNavigation?.openInbox?.invoke(section, null)
                                                },
                                                onApprove = { peer -> act { networking?.approveContact(peer); networking?.sync() } },
                                                onSelect = { contact ->
                                                    if (contact != null && platformNavigation != null) platformNavigation.openInbox(InboxPage.Conversation, contact)
                                                    else dmContact = contact
                                                }, onClose = { if (mailSection != InboxPage.Messages) mailSection = InboxPage.Messages else inbox = false }, onBrowse = { navigate("Nearby") }, onJoin = ::openCommunitySettings,
                                                onRefresh = { if (live && (connection.directAvailable || connection.mailboxElsewhere)) act { networking.refreshBlocks() } },
                                                onBlock = { blocking = BlockChoice(it.peer, it.label, false) },
                                                onUnblock = { id ->
                                                    if (live) act { networking.unblockAccount(id); networking.sync() }
                                                    else change { repository.setDemoBlock(id, null) }
                                                },
                                                onSend = { peer, body ->
                                                    try {
                                                        val controller = checkNotNull(networking)
                                                        if (platformNavigation != null && inboxPage != null && !platformNavigation.matchesInboxScope(controller.state.value)) {
                                                            return@InboxScreen false
                                                        }
                                                        controller.sendMessage(peer, body)
                                                        scope.launch { controller.sync() }
                                                        true
                                                    } catch (failure: Exception) {
                                                        if (failure is CancellationException) throw failure
                                                        error = failure.message ?: "Couldn’t send message."
                                                        false
                                                    }
                                                })
                                            visibleTab == "Groups" -> GroupsScreen(connection,
                                                privateGroups = networking,
                                                openPrivateGroups = platformNavigation?.openPrivateGroups ?: materialNavigation?.let { { privateGroupsOpen = true } },
                                                onRefresh = if (canRefresh) ::refreshContent else null,
                                                onConnect = ::openCommunitySettings,
                                                onCity = { city -> act { networking?.selectLocality(city) } },
                                                onOpen = { id -> networking?.openGroup(id); returnToNearby(); scope.launch { networking?.sync() } },
                                                onJoin = { group -> networking?.joinGroup(group); returnToNearby(); scope.launch { networking?.sync() } },
                                                onCreate = { name, description, category, locality ->
                                                    networking?.createGroup(name, description, category, locality)
                                                    returnToNearby(); scope.launch { networking?.sync() }
                                                }, onError = { error = it },
                                                onHome = { networking?.openHomeFeed(); returnToNearby(); scope.launch { networking?.sync() } })
                                            visibleTab == "You" -> Profile(state.copy(profile = demoState.profile), live = live, connection = connection, networking = networking,
                                                recoveryFiles = recoveryFiles, listState = profileScroll,
                                                settingsOnly = platformNavigation?.profileSettings == true, onSettings = platformNavigation?.openProfileSettings,
                                                onPrivacy = {
                                                    if (platformNavigation != null) platformNavigation.openInbox(InboxPage.Privacy, null)
                                                    else privacyOpen = true
                                                }, onGuide = platformNavigation?.openGuide ?: { revisitGuide = true },
                                                onEdit = { editingProfile = true }, onProfile = ::saveProfile,
                                                onCompose = { postDraft = PostDraft() }, onReset = { resetConfirm = true }, onPost = { openConversation(it) },
                                                onJoin = { invitation -> networking!!.connect(invitation, demoState.alias); returnToNearby(); scope.launch { networking.sync() } },
                                                onDemo = { act { networking!!.useDemo(); selectedId = null } }, onSync = { act { networking!!.sync() } },
                                                onDiscard = { act { networking!!.discardPending(); networking.sync() } }, onError = { error = it })
                                            else -> {
                                                val saved = visibleTab == "Saved"
                                                LazyColumn(Modifier.fillMaxSize(), state = if (saved) savedScroll else nearbyScroll,
                                                    contentPadding = PaddingValues(bottom = if (feedFab) 88.dp else 0.dp)) {
                                                    item {
                                                        val group = connection.activeGroup
                                                        val feedTag = if (live && connection.online && connection.pendingCount == 0) "updated-community-feed" else "feed-title"
                                                        Row(Modifier.fillMaxWidth().padding(horizontal = 22.dp),
                                                            horizontalArrangement = Arrangement.spacedBy(if (feedButton != null) 12.dp else 0.dp),
                                                            verticalAlignment = Alignment.CenterVertically) {
                                                            if (saved || group != null) Column(Modifier.weight(1f)) {
                                                                Text(if (saved) "Saved posts" else group!!.name, Modifier.testTag(feedTag),
                                                                    style = MaterialTheme.typography.headlineSmall, fontWeight = FontWeight.Bold)
                                                                if (!saved) Text("${group?.locality?.label ?: "Worldwide"} · Public group", style = MaterialTheme.typography.bodySmall)
                                                            } else if (feedButton != null) {
                                                                feedButton(FormRow("location", nearby.center?.label ?: "All cities", "location",
                                                                    detail = if (nearby.center != null) "Within ${nearby.radiusMiles} miles" else "",
                                                                    identifier = feedTag, symbol = "chevron.down", onTap = { locationInfo = true }), Modifier.weight(1f))
                                                            } else Row(Modifier.weight(1f).testTag(feedTag).heightIn(min = 48.dp).clickable(
                                                                role = Role.Button, onClickLabel = "Change location", onClick = { locationInfo = true }),
                                                                verticalAlignment = Alignment.CenterVertically) {
                                                                Column(Modifier.weight(1f, fill = false)) {
                                                                    Text(nearby.center?.label ?: "All cities",
                                                                        style = MaterialTheme.typography.headlineSmall, fontWeight = FontWeight.Bold)
                                                                    if (nearby.center != null) Text("Within ${nearby.radiusMiles} miles", style = MaterialTheme.typography.bodySmall)
                                                                }
                                                                LineIcon("down", null, Modifier.padding(start = 4.dp).size(16.dp))
                                                            }
                                                            if (!saved && feedButton != null) {
                                                                val count = (if (popular) 1 else 0) + (if (topic != "All") 1 else 0)
                                                                feedButton(FormRow("filter", if (count == 0) "Filter" else "Filter ($count)",
                                                                    symbol = "line.3.horizontal.decrease", onTap = { filtersOpen = true }), Modifier)
                                                            } else if (!saved) YapOutlinedButton(onClick = { filtersOpen = true },
                                                                contentPadding = PaddingValues(horizontal = 12.dp)) {
                                                                val count = (if (popular) 1 else 0) + (if (topic != "All") 1 else 0)
                                                                if (materialNavigation != null) ActionIcon("filter")
                                                                Text(if (count == 0) "Filter" else "Filter ($count)")
                                                            }
                                                        }
                                                        if (!saved && topic != "All") Text("Topic · $topic",
                                                            Modifier.padding(start = 22.dp, end = 22.dp, bottom = 4.dp),
                                                            style = MaterialTheme.typography.bodySmall, color = colors.primary)
                                                    }
                                                    if (!live && !connection.awaitingDefault) item {
                                                        Text("Demo neighborhood", Modifier.padding(horizontal = 22.dp, vertical = 8.dp), style = MaterialTheme.typography.labelMedium)
                                                    }
                                                    if (connection.feedUnavailable && (live || connection.awaitingDefault)) item {
                                                        Row(Modifier.fillMaxWidth().padding(horizontal = 22.dp), verticalAlignment = Alignment.CenterVertically) {
                                                            val message = when {
                                                                state.replicaAsOf != null -> "Showing the last available posts."
                                                                state.posts.isEmpty() -> "Couldn’t load posts."
                                                                else -> "Couldn’t refresh. Showing saved posts."
                                                            }
                                                            Text(message, Modifier.weight(1f), style = MaterialTheme.typography.bodyMedium)
                                                            if (feedButton != null) feedButton(FormRow("retry", "Try again", enabled = !refreshing,
                                                                onTap = ::refreshContent), Modifier)
                                                            else TextButton(enabled = !refreshing, onClick = ::refreshContent) { Text("Try again") }
                                                        }
                                                    }
                                                    val posts = visiblePosts(state, if (saved) "All" else topic, popular && !saved, saved).filter {
                                                        saved || (live && connection.activeGroup != null) || nearby.includes(it.area)
                                                    }
                                                    if (feedButton != null && connection.awaitingDefault && posts.isEmpty() && !connection.feedUnavailable) item(key = "feed-loading") {
                                                        Text("Loading posts…", Modifier.padding(horizontal = 22.dp, vertical = 12.dp)
                                                            .semantics { liveRegion = LiveRegionMode.Polite }, style = MaterialTheme.typography.bodyMedium,
                                                            color = colors.onSurfaceVariant)
                                                    }
                                                    val filtered = policy.settings.hideAnonymous || policy.settings.feedDays > 0 || policy.mutedThreads.isNotEmpty()
                                                    val topicEmpty = !saved && topic != "All" && posts.isEmpty() && !connection.awaitingDefault && !connection.feedUnavailable
                                                    if (topicEmpty) item {
                                                        EmptyState("No posts in $topic", "Try another topic.", "Show all topics") { topic = "All" }
                                                    }
                                                    if (!topicEmpty && posts.isEmpty() && !connection.awaitingDefault && (saved || !connection.feedUnavailable) && filtered) item {
                                                        EmptyState("No posts to show.", "Your privacy and feed filters are on.", "Review privacy") {
                                                            if (platformNavigation != null) platformNavigation.openInbox(InboxPage.Privacy, null)
                                                            else privacyOpen = true
                                                        }
                                                    }
                                                    if (!topicEmpty && posts.isEmpty() && !connection.awaitingDefault && (saved || !connection.feedUnavailable) && !filtered) item {
                                                        EmptyState(if (saved) "No saved posts yet" else "No posts yet",
                                                            if (saved) "Tap the bookmark on a post to save it here." else if (nearby.center != null) "Try a wider radius or a different town." else "Start a conversation in your neighborhood.",
                                                            if (saved) "Explore nearby" else if (nearby.center != null && connection.activeGroup == null) "Change location" else "Write a post",
                                                            icon = if (saved) "bookmark" else "reply") { if (saved) navigate("Nearby") else if (nearby.center != null && connection.activeGroup == null) locationInfo = true else postDraft = PostDraft() }
                                                    }
                                                    items(posts, key = { it.id }) { post ->
                                                        PostRow(post, onOpen = { openConversation(post.id) }, onComment = { openConversation(post.id, atComments = true) },
                                                            onVote = { vote(post, it) },
                                                            onReport = openReport, onMute = { mutePost(post) }, onSave = { save(post) }, onHide = { hidePostId = post.id }, onDelete = { deleteTarget = it },
                                                            onAuthor = { showAuthor(post) }, stackUsername = platformNavigation != null)
                                                    }
                                                }
                                            }
                                        }
                                    }
                                    showingThread?.let { selected ->
                                        if (sideBySide) VerticalDivider(color = colors.outlineVariant)
                                        Box(Modifier.weight(1f).fillMaxHeight().then(
                                            if (sideBySide) Modifier.background(colors.surfaceContainerLow) else Modifier
                                        )) {
                                            key(selected.id, openAtComments) {
                                                ThreadContent(selected, dedicated = false)
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
        if (materialNavigation == null) Screen(null) else {
            materialNavigation.Pages(materialStack, ::goBack) { Screen(it) }
        }
        if (editingProfile && nativeForms) ProfileEditor(state.alias, demoState.profile, photoLibrary,
            onDismiss = { editingProfile = false }, onSave = ::saveProfile)
        postDraft?.let { draft ->
            ComposeSheet(draft, namedIdentity, anonymous, ::chooseIdentity,
                destination = connection.activeGroup?.name ?: nearby.center?.label ?: "All cities",
                networked = live, needsUsername = live && connection.needsUsername, usernameClaim = editorUsernameClaim,
                usernameControl = { UsernameControl(connection, networking) }, onDismiss = { postDraft = null }
            ) { headline, body, category, asAnonymous ->
                if (live) { networking.publish(body, category, asAnonymous, state.alias, headline, demoState.profile, nearby.center.takeUnless { connection.activeGroup != null }); scope.launch { networking.sync() } }
                else { repository.publish(body, category, asAnonymous, headline); demoState = repository.snapshot }
                postDraft = null
                inform(if (live) "Post saved." else "Post added to the demo.")
                returnToNearby()
            }
        }
        shownAuthor?.let { author ->
            val event = author.event
            val actionable = !author.mine && (!live || event != null)
            AuthorProfileSheet(author.name, author.username, author.profile, onDismiss = { shownAuthor = null },
                onMessage = if (live && connection.directAvailable && connection.messagingScope != null && actionable && event != null && author.peer != null) ({ contact(event) }) else null,
                onBlock = if (actionable) ({
                    shownAuthor = null
                    blocking = BlockChoice(event ?: author.localKey, author.name, true, author.peer)
                }) else null)
        }
        blocking?.let { choice ->
            PlatformConfirmation("Block ${choice.label}?", when {
                    !live -> "Hide this account’s posts and replies in your local demo."
                    choice.byEvent && choice.peer == null -> "This post or reply will disappear on this device. When you sync, your home will hide the account’s other posts and replies and stop messages between you."
                    else -> "Hide their posts, replies, and conversation on this device. When you sync, your home will also stop messages between you."
                }, "Block account", onCancel = { blocking = null }) {
                    blocking = null
                    act {
                        if (live) networking.blockAccount(choice.id, choice.byEvent, choice.label, choice.peer)
                        else { repository.setDemoBlock(choice.id, choice.label); demoState = repository.snapshot }
                        inform(if (live && !connection.online) "Block saved. Your home will apply it when you sync." else "Account blocked.")
                        closeConversation(); dmContact = null
                        if (live) networking.sync()
                    }
                }
        }
        if (filtersOpen) FeedFilterDialog(topic, popular, onDismiss = { filtersOpen = false }) { choice, popularFirst ->
            topic = choice
            popular = popularFirst
            filtersOpen = false
        }
        if (locationInfo) NearbySheet(nearby, nearbyServices, onDismiss = { locationInfo = false }) { preference ->
            if (change { repository.setNearby(preference) }) {
                locationInfo = false
                act { networking?.setNearby(preference); if (live) networking.sync() }
            }
        }
        deleteTarget?.let { target ->
            PlatformConfirmation("Delete this from the community?", "Your home will erase this item and send the deletion to other homes when they sync.",
                "Delete", onCancel = { deleteTarget = null }) {
                deleteTarget = null
                act { networking?.delete(target); networking?.sync() }
            }
        }
        if (live) reportTarget?.let { ReportDialog(it, networking) { reportTarget = null } }
        hidePostId?.let { id ->
            PlatformConfirmation("Hide this post on this device?", "Other people can still see this post.", "Hide post", onCancel = { hidePostId = null }) {
                act {
                    if (live) networking.hide(id) else { repository.hide(id); demoState = repository.snapshot }
                    hidePostId = null
                    inform("Post hidden on this device.")
                    closeConversation()
                }
            }
        }
        if (resetConfirm) PlatformConfirmation("Reset local demo?", "This removes your posts, replies, saves, and votes and restores the sample feed.",
            "Reset demo", onCancel = { resetConfirm = false }) { change { repository.reset() }; resetConfirm = false }
        error?.let { message ->
            PlatformConfirmation("Couldn't save that", message, "OK", onCancel = { error = null }, cancel = "", destructive = false) { error = null }
        }
        if (materialNavigation != null) {
            LaunchedEffect(notice) {
                notice?.let { message ->
                    snackbars.showSnackbar(message)
                    notice = null
                }
            }
        } else notice?.let { message -> LaunchedEffect(message) { kotlinx.coroutines.delay(2600); notice = null }
            Box(Modifier.fillMaxSize().safeDrawingPadding().padding(bottom = 100.dp), contentAlignment = Alignment.BottomCenter) {
                Surface(shape = RoundedCornerShape(14.dp), color = colors.inverseSurface, modifier = Modifier.padding(20.dp)) {
                    Text(message, Modifier.padding(16.dp), color = colors.inverseOnSurface, style = MaterialTheme.typography.bodyMedium)
                }
            }
        }
    }
}

@Composable
private fun AppFrame(nativeNavigation: Boolean, floatingActionButton: @Composable () -> Unit,
                     snackbarHost: @Composable () -> Unit, bottomBar: @Composable () -> Unit,
                     content: @Composable (PaddingValues) -> Unit) {
    val colors = MaterialTheme.colorScheme
    if (nativeNavigation) {
        Box(Modifier.fillMaxSize().background(colors.background)) {
            CompositionLocalProvider(LocalContentColor provides colors.onBackground) {
                content(WindowInsets.systemBars.asPaddingValues())
            }
        }
    } else {
        Scaffold(modifier = Modifier.fillMaxSize(), containerColor = colors.background,
            floatingActionButton = floatingActionButton, snackbarHost = snackbarHost, bottomBar = bottomBar, content = content)
    }
}

internal fun isAnonymous(name: String, visibility: AuthorVisibility?) =
    visibility?.anonymous ?: (name.isBlank() || name == "Anonymous neighbor")

private data class SharedAuthor(
    val name: String, val username: String?, val profile: CommunityProfile?, val event: String?,
    val localKey: String, val mine: Boolean, val peer: String?
)

@Composable
private fun PostByline(name: String, username: String?, anonymous: Boolean, modifier: Modifier = Modifier,
                       stackUsername: Boolean = false, onOpen: () -> Unit) {
    val label = if (anonymous) "Anonymous neighbor" else name.ifBlank { username ?: "Neighbor" }
    val native = LocalFeedControl.current
    if (!anonymous && native != null) {
        native(FormRow("author", label, "byline", detail = username.orEmpty(), onTap = onOpen), modifier)
        return
    }
    val colors = MaterialTheme.colorScheme
    val target = if (anonymous) Modifier else Modifier.heightIn(min = 48.dp)
        .clickable(role = Role.Button, onClickLabel = "View profile", onClick = onOpen)
        .semantics { contentDescription = "View ${label}'s profile" }
    val nameLabel: @Composable () -> Unit = {
        Text(label, style = MaterialTheme.typography.labelLarge, fontWeight = FontWeight.SemiBold,
            color = if (anonymous) colors.onSurface else colors.primary,
            textDecoration = if (anonymous) null else TextDecoration.Underline)
    }
    val usernameLabel: @Composable () -> Unit = {
        if (!anonymous && !username.isNullOrBlank()) Text(username, style = MaterialTheme.typography.bodySmall, color = colors.onSurfaceVariant)
    }
    if (stackUsername) {
        Column(modifier.then(target), verticalArrangement = Arrangement.Center) {
            nameLabel()
            usernameLabel()
        }
    } else {
        FlowRow(modifier, horizontalArrangement = Arrangement.spacedBy(8.dp), itemVerticalAlignment = Alignment.CenterVertically) {
            Box(target, contentAlignment = Alignment.CenterStart) { nameLabel() }
            usernameLabel()
        }
    }
}

private fun postTime(post: Post): String? {
    val minutes = post.acceptedAt?.let {
        ((kotlin.time.Clock.System.now().epochSeconds - it) / 60).coerceAtLeast(0)
    } ?: post.minutesAgo.toLong().takeIf { post.remoteId == null }
    return when {
        post.pending -> "Waiting to send"
        minutes == null -> null
        minutes == 0L -> "Just now"
        minutes < 60 -> "${minutes}m ago"
        minutes < 1440 -> "${minutes / 60}h ago"
        else -> "${minutes / 1440}d ago"
    }
}

internal val LocalRepliesButton = staticCompositionLocalOf<@Composable (Int, () -> Unit) -> Unit> {
    { count, open ->
        YapFilledTonalButton(onClick = open, contentPadding = PaddingValues(horizontal = 12.dp)) {
            LineIcon("reply", null, Modifier.size(18.dp), tint = LocalContentColor.current)
            Spacer(Modifier.width(6.dp))
            Text("Replies ($count)")
        }
    }
}

@Composable
private fun PostToggle(icon: String, description: String, checked: Boolean, onClick: () -> Unit) {
    val colors = MaterialTheme.colorScheme
    YapIconToggle(checked = checked, onClick = onClick,
        colors = IconButtonDefaults.filledTonalIconToggleButtonColors(
            containerColor = Color.Transparent, contentColor = colors.onSurfaceVariant,
            checkedContainerColor = colors.primary, checkedContentColor = colors.onPrimary
        )) {
        LineIcon(icon, description, tint = LocalContentColor.current, filled = checked)
    }
}

@Composable
private fun PostRow(post: Post, onReport: ((ReportTarget) -> Unit)?, onOpen: (() -> Unit)?, onVote: (Int) -> Unit,
                    onSave: () -> Unit, onMute: () -> Unit, onHide: () -> Unit, onDelete: (String) -> Unit,
                    onAuthor: () -> Unit, onComment: () -> Unit, stackUsername: Boolean = false) {
    val colors = MaterialTheme.colorScheme
    val native = LocalFeedControl.current
    val options = buildList {
        add(FormChoice(if (post.saved) "Unsave post" else "Save post", action = onSave))
        add(FormChoice("Mute thread", action = onMute))
        if (post.mine && !post.pending && post.remoteId != null) add(FormChoice("Delete post", destructive = true) { onDelete(post.remoteId) })
        if (onReport != null && post.remoteId != null) add(FormChoice("Report post") {
            onReport(ReportTarget("post", post.remoteId, listOf(post.headline, post.body).filter { it.isNotBlank() }.joinToString("\n")))
        })
        add(FormChoice("Hide on this device", action = onHide))
    }
    var menu by remember { mutableStateOf(false) }
    Column(Modifier.fillMaxWidth().padding(horizontal = 22.dp, vertical = 10.dp)) {
        Row(verticalAlignment = Alignment.CenterVertically) {
            PostByline(post.author, post.username, isAnonymous(post.author, post.visibility), Modifier.weight(1f), stackUsername, onAuthor)
            if (native != null) native(FormRow("options", "Post options", "menu", symbol = "ellipsis", choices = options), Modifier)
            else Box {
                IconButton(onClick = { menu = true }) { LineIcon("more", "Post options") }
                DropdownMenu(expanded = menu, onDismissRequest = { menu = false }) {
                    options.forEach { action -> DropdownMenuItem(text = { Text(action.title) }, onClick = { menu = false; action.select() }) }
                }
            }
        }
        if (native != null && onOpen != null) {
            native(FormRow("post", post.headline, "story", detail = post.body, onTap = onOpen), Modifier.fillMaxWidth().padding(bottom = 8.dp))
        } else {
            val openModifier = if (onOpen == null) Modifier else Modifier.clickable(onClickLabel = "Open conversation", onClick = onOpen)
            Column(Modifier.fillMaxWidth().then(openModifier).padding(bottom = 8.dp)) {
                if (post.headline.isNotBlank()) Text(post.headline, Modifier.padding(bottom = 6.dp),
                    style = postTitleStyle, fontWeight = FontWeight.SemiBold)
                Text(post.body, style = postBodyStyle)
            }
        }
        val location = post.area?.label ?: post.distance.takeIf { post.remoteId == null && !post.pending }
        Text(listOfNotNull(post.topic, location, postTime(post)).joinToString(" · "),
            Modifier.padding(bottom = 4.dp), style = MaterialTheme.typography.bodySmall, color = colors.onSurfaceVariant)
        Row(Modifier.fillMaxWidth(), verticalAlignment = Alignment.CenterVertically) {
            FlowRow(Modifier.weight(1f), horizontalArrangement = Arrangement.spacedBy(if (native != null) 10.dp else 4.dp),
                itemVerticalAlignment = Alignment.CenterVertically) {
                if (LocalMaterialNavigation.current != null && onOpen != null) {
                    val count = post.replies.size
                    YapFilledTonalButton(onClick = onComment, contentPadding = PaddingValues(horizontal = 12.dp),
                        modifier = Modifier.semantics { contentDescription = "Replies ($count)" }) {
                        LineIcon("reply", null, Modifier.size(18.dp), tint = LocalContentColor.current)
                        Spacer(Modifier.width(6.dp))
                        Text(count.toString(), Modifier.clearAndSetSemantics {})
                    }
                } else LocalRepliesButton.current(post.replies.size, onComment)
                if (LocalMaterialNavigation.current != null) VoteControl(post.score, post.vote, onVote)
                else Row(verticalAlignment = Alignment.CenterVertically) {
                    if (native != null) native(FormRow("upvote", "Upvote", "icon", symbol = "chevron.up", checked = post.vote == 1,
                        onTap = { onVote(1) }), Modifier)
                    else IconToggleButton(checked = post.vote == 1, onCheckedChange = { onVote(1) }) {
                        LineIcon("up", "Upvote", Modifier.size(16.dp), tint = if (post.vote == 1) colors.primary else colors.onSurfaceVariant)
                    }
                    Text("${post.score}", style = MaterialTheme.typography.labelMedium, color = colors.onSurfaceVariant)
                    if (native != null) native(FormRow("downvote", "Downvote", "icon", symbol = "chevron.down", checked = post.vote == -1,
                        onTap = { onVote(-1) }), Modifier)
                    else IconToggleButton(checked = post.vote == -1, onCheckedChange = { onVote(-1) }) {
                        LineIcon("down", "Downvote", Modifier.size(16.dp), tint = if (post.vote == -1) colors.primary else colors.onSurfaceVariant)
                    }
                }
            }
            if (native != null) native(FormRow("save", if (post.saved) "Unsave post" else "Save post", "icon",
                symbol = if (post.saved) "bookmark.fill" else "bookmark", checked = post.saved, onTap = onSave), Modifier)
            else if (LocalMaterialNavigation.current != null) PostToggle("bookmark", if (post.saved) "Unsave post" else "Save post", post.saved, onSave)
            else IconButton(onClick = onSave) {
                LineIcon("bookmark", if (post.saved) "Unsave post" else "Save post",
                    tint = if (post.saved) colors.primary else colors.onSurfaceVariant, filled = post.saved)
            }
        }
    }
    HorizontalDivider(color = colors.outlineVariant)
}

/** Reveal only the comment submitted here; background sync must not interrupt reading. */
private data class BlockChoice(val id: String, val label: String, val byEvent: Boolean, val peer: String? = null)

private data class CommentReveal(val previousIds: Set<Long>, val body: String)

@Composable
private fun Conversation(post: Post, alias: String, namedIdentity: String, anonymous: Boolean,
                         chooseIdentity: (Boolean) -> Unit, networked: Boolean, backLabel: String, openAtComments: Boolean, onBack: () -> Unit, onReply: suspend (String, Boolean) -> Boolean,
                         onReport: ((ReportTarget) -> Unit)?, onVote: (Int) -> Unit, onSave: () -> Unit, onMute: () -> Unit, onHide: () -> Unit, onDelete: (String) -> Unit, onAuthor: () -> Unit,
                         needsUsername: Boolean, usernameControl: @Composable () -> Unit, editorUsernameClaim: EditorUsernameClaim?, replyActionsEnabled: Boolean,
                         onReplyActions: (Reply) -> Unit, onReplyAuthor: (Reply) -> Unit, dedicated: Boolean,
                         refreshReplies: (suspend () -> Unit)?) {
    var body by remember(post.id) { mutableStateOf("") }
    var sending by remember(post.id) { mutableStateOf(false) }
    var writing by remember(post.id) { mutableStateOf(false) }
    var replyFocused by remember(post.id) { mutableStateOf(false) }
    var reveal by remember(post.id) { mutableStateOf<CommentReveal?>(null) }
    val commentFocus = remember { FocusRequester() }
    val list = rememberLazyListState(initialFirstVisibleItemIndex = if (openAtComments) 1 else 0)
    val focus = LocalFocusManager.current
    val keyboard = LocalSoftwareKeyboardController.current
    val imeBottom = WindowInsets.ime.getBottom(LocalDensity.current)
    val scope = rememberCoroutineScope()
    val colors = MaterialTheme.colorScheme
    LaunchedEffect(post.id, post.replies.map { it.id }, reveal, imeBottom) {
        val request = reveal ?: return@LaunchedEffect
        if (imeBottom > 0) return@LaunchedEffect
        val index = post.replies.indexOfLast {
            it.id !in request.previousIds && (it.mine || it.pending || it.author == alias) && it.body == request.body
        }
        if (index >= 0) {
            // The post and Comments heading precede the keyed comment rows.
            withFrameNanos { }
            list.animateScrollToItem(index + 2)
            reveal = null
        }
    }
    fun closeEditor() {
        focus.clearFocus()
        keyboard?.hide()
        writing = false
    }
    val nativeButton = LocalFeedControl.current
    val nativeEditor = LocalInlineEditor.current
    var dismissKeyboardRequest by remember(post.id) { mutableIntStateOf(0) }
    fun canReply() = body.isNotBlank() && !sending && (anonymous || !needsUsername)
    fun sendReply() {
        if (!canReply()) return
        val draft = body
        val identity = anonymous
        val before = post.replies.map { it.id }.toSet()
        sending = true
        scope.launch {
            try {
                if (onReply(draft, identity)) {
                    reveal = CommentReveal(before, draft.trim())
                    body = ""
                    if (nativeEditor != null) dismissKeyboardRequest++ else closeEditor()
                }
            } finally { sending = false }
        }
    }
    Column(Modifier.fillMaxSize()) {
        if (!dedicated) {
            val replyAction: @Composable () -> Unit = {
                TextButton(enabled = !sending, onClick = { if (writing) closeEditor() else writing = true }) {
                    Text(if (writing) "Hide editor" else if (body.isNotEmpty()) "Continue reply" else "Write a reply")
                }
            }
            val navigation = LocalMaterialNavigation.current
            if (navigation != null) navigation.UpBar("Conversation", onBack) { replyAction() }
            else Row(Modifier.fillMaxWidth().padding(start = 10.dp, end = 14.dp), verticalAlignment = Alignment.CenterVertically) {
                TextButton(onClick = onBack) { LineIcon("back", null); Text(backLabel) }
                Spacer(Modifier.weight(1f))
                replyAction()
            }
        }
        var refreshingReplies by remember(post.id) { mutableStateOf(false) }
        fun refreshThread() {
            if (refreshReplies == null || refreshingReplies) return
            refreshingReplies = true
            scope.launch {
                try { refreshReplies() }
                finally { refreshingReplies = false }
            }
        }
        @Composable
        fun ReplyList(modifier: Modifier) {
            LazyColumn(modifier, state = list) {
                item(key = "post") { PostRow(post, onReport, null, onVote, onSave, onMute, onHide, onDelete, onAuthor, onComment = {
                    scope.launch { list.animateScrollToItem(1) }
                }) }
                item(key = "comments") {
                    Row(Modifier.fillMaxWidth().padding(start = 22.dp, end = 22.dp, top = 22.dp, bottom = 10.dp), verticalAlignment = Alignment.CenterVertically) {
                        Text("Comments", style = MaterialTheme.typography.titleLarge, fontWeight = FontWeight.Bold)
                        Text("${post.replies.size}", Modifier.padding(start = 8.dp), color = colors.onSurfaceVariant, style = MaterialTheme.typography.titleMedium)
                        if (refreshReplies != null && nativeButton != null) {
                            Spacer(Modifier.weight(1f))
                            nativeButton(FormRow("refresh-replies", "Refresh replies", "icon", symbol = "arrow.clockwise",
                                enabled = !refreshingReplies, onTap = ::refreshThread), Modifier)
                        }
                    }
                }
                if (post.replies.isEmpty()) item(key = "empty") {
                    Text("No comments yet.", Modifier.padding(horizontal = 22.dp, vertical = 12.dp), color = colors.onSurfaceVariant)
                }
                items(post.replies, key = { it.id }) { reply ->
                    Column(Modifier.fillMaxWidth().padding(horizontal = 22.dp, vertical = 14.dp)) {
                        Row(verticalAlignment = Alignment.CenterVertically) {
                            PostByline(reply.author, reply.username, isAnonymous(reply.author, reply.visibility), Modifier.weight(1f)) { onReplyAuthor(reply) }
                            if (networked && reply.mine && !reply.pending && reply.remoteId != null) {
                                if (nativeButton != null) nativeButton(FormRow("delete", "Delete reply", symbol = "trash", destructive = true,
                                    onTap = { onDelete(reply.remoteId) }), Modifier)
                                else TextButton(onClick = { onDelete(reply.remoteId) }) { Text("Delete reply") }
                            }
                            if (replyActionsEnabled && !reply.mine && !reply.pending && (reply.remoteId != null || reply.id in 20L..22L)) {
                                var options by remember(reply.id) { mutableStateOf(false) }
                                val actions = buildList {
                                    if (onReport != null && reply.remoteId != null) add(FormChoice("Report reply") { onReport(ReportTarget("reply", reply.remoteId, reply.body)) })
                                    add(FormChoice("Block account", destructive = true) { onReplyActions(reply) })
                                }
                                if (nativeButton != null) nativeButton(FormRow("reply-options", "Reply options for ${reply.author}",
                                    "menu", symbol = "ellipsis", choices = actions), Modifier)
                                else Box {
                                    IconButton(onClick = { options = true }) { LineIcon("more", "Reply options for ${reply.author}") }
                                    DropdownMenu(options, onDismissRequest = { options = false }) {
                                        actions.forEach { action -> DropdownMenuItem(text = { Text(action.title) }, onClick = { options = false; action.select() }) }
                                    }
                                }
                            }
                            if (reply.pending) Text("Waiting to sync", style = MaterialTheme.typography.labelSmall, color = colors.onSurfaceVariant)
                        }
                        Text(reply.body, Modifier.padding(top = 7.dp), style = postBodyStyle)
                    }
                    HorizontalDivider(Modifier.padding(horizontal = 22.dp), color = colors.outlineVariant)
                }
            }
        }
        val listModifier = Modifier.weight(1f).fillMaxWidth()
        if (refreshReplies == null) ReplyList(listModifier)
        else FeedRefreshPane(listModifier, enabled = true, refreshing = refreshingReplies, onRefresh = ::refreshThread) {
            ReplyList(Modifier.fillMaxSize())
        }
        if (dedicated && nativeEditor != null) {
            Box(Modifier.fillMaxWidth().imePadding()) {
                nativeEditor(InlineEditorModel(
                    currentBody = { body }, limit = POST_LIMIT, isSending = { sending }, sendEnabled = ::canReply,
                    dismissKeyboardRequest = dismissKeyboardRequest, namedIdentity = namedIdentity, anonymous = anonymous,
                    networked = networked, usernameClaim = editorUsernameClaim.takeIf { !anonymous && needsUsername },
                    onEdit = { body = it }, onSend = ::sendReply, onChooseIdentity = chooseIdentity,
                ))
            }
        } else if (writing || dedicated) Column(Modifier.fillMaxWidth().imePadding().background(colors.background)) {
            if (!dedicated) LaunchedEffect(post.id) {
                commentFocus.requestFocus()
                keyboard?.show()
            }
            HorizontalDivider(color = colors.outlineVariant)
            Column(Modifier.padding(horizontal = 22.dp, vertical = 8.dp), verticalArrangement = Arrangement.spacedBy(4.dp)) {
                val composingReply = !dedicated || replyFocused || body.isNotEmpty() || sending
                OutlinedTextField(value = body, textStyle = postBodyStyle, onValueChange = { if (!sending && it.length <= POST_LIMIT) body = it },
                    label = { Text(if (dedicated) "Write a reply" else "Your comment") },
                    placeholder = { if (!dedicated) Text("Add to the conversation…") },
                    modifier = Modifier.fillMaxWidth().then(if (dedicated) Modifier else Modifier.heightIn(min = 104.dp))
                        .focusRequester(commentFocus).onFocusChanged { replyFocused = it.hasFocus },
                    minLines = if (dedicated) 1 else 3, maxLines = if (dedicated) 4 else 5, readOnly = sending,
                    supportingText = if (composingReply) ({ Text("${body.length} / $POST_LIMIT") }) else null)
                if (composingReply && !anonymous && needsUsername) usernameControl()
                if (composingReply) Row(Modifier.fillMaxWidth(), verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(12.dp)) {
                    IdentityPicker("Reply", namedIdentity, anonymous, networked, !sending, chooseIdentity, Modifier.weight(1f))
                    YapButton(enabled = canReply(), onClick = ::sendReply) { Text(if (sending) "Posting…" else "Post comment") }
                }
            }
        }
    }
}

@Composable
private fun IdentityPicker(action: String, namedIdentity: String, anonymous: Boolean, networked: Boolean,
                           enabled: Boolean, onSelect: (Boolean) -> Unit, modifier: Modifier = Modifier) {
    var open by remember { mutableStateOf(false) }
    Box(modifier) {
        TextButton(enabled = enabled, onClick = { open = true }) {
            Text("$action as ${if (anonymous) "Anonymous" else namedIdentity}", Modifier.weight(1f, fill = false))
            LineIcon("down", null, Modifier.size(16.dp))
        }
        if (open) AlertDialog(onDismissRequest = { open = false }, title = { Text("Posting identity") },
            text = { Column {
                Text("Use for posts and replies", style = MaterialTheme.typography.bodyMedium)
                listOf(true to "Anonymous", false to namedIdentity).forEach { (asAnonymous, label) ->
                    Row(Modifier.fillMaxWidth().then(if (asAnonymous) Modifier else Modifier.testTag("named-posting-identity"))
                        .selectable(selected = anonymous == asAnonymous, role = Role.RadioButton,
                            onClick = { onSelect(asAnonymous); open = false }).heightIn(min = 48.dp),
                        verticalAlignment = Alignment.CenterVertically) {
                        RadioButton(selected = anonymous == asAnonymous, onClick = null)
                        Text(label, Modifier.padding(start = 12.dp))
                    }
                }
                if (networked) Text("Your home can identify anonymous posts.", style = MaterialTheme.typography.bodySmall)
            } }, confirmButton = { YapCancelButton(onClick = { open = false }) })
    }
}

@Composable
private fun FeedFilterDialog(topic: String, popular: Boolean, onDismiss: () -> Unit, onApply: (String, Boolean) -> Unit) {
    var selected by remember { mutableStateOf(topic) }
    var popularFirst by remember { mutableStateOf(popular) }
    val nativeForm = LocalNativeForm.current
    if (nativeForm != null) {
        val sort = FormRow("sort", "Sort by", "segments", choices = listOf(
            FormChoice("New", selected = !popularFirst) { popularFirst = false },
            FormChoice("Popular", selected = popularFirst) { popularFirst = true }))
        val choices = topics.map { choice -> FormRow(choice, choice, "choice", checked = selected == choice, onTap = { selected = choice }) }
        nativeForm(NativeFormModel("Feed filters", listOf(FormSection(rows = listOf(sort)), FormSection("Topic", choices)),
            leading = FormChoice("Cancel", action = onDismiss), trailing = FormChoice("Apply") { onApply(selected, popularFirst) },
            onDismiss = onDismiss), "sheet", Modifier)
        return
    }
    AlertDialog(onDismissRequest = onDismiss, title = { Text("Feed filters") }, text = {
        LazyColumn(verticalArrangement = Arrangement.spacedBy(8.dp)) {
            item { Text("Sort by", style = MaterialTheme.typography.titleSmall) }
            item { YapSegments(listOf("New", "Popular"), if (popularFirst) "Popular" else "New") { popularFirst = it == "Popular" } }
            item { Text("Topic", style = MaterialTheme.typography.titleSmall) }
            item { FlowRow(horizontalArrangement = Arrangement.spacedBy(6.dp)) {
                topics.forEach { choice ->
                    YapFilterChip(selected = selected == choice, onClick = { selected = choice }, label = { Text(choice) })
                }
            } }
        }
    }, confirmButton = { YapButton(onClick = { onApply(selected, popularFirst) }) { Text("Apply") } },
        dismissButton = { YapCancelButton(onClick = onDismiss) })
}

@Composable
private fun ComposeSheet(draft: PostDraft, namedIdentity: String, anonymous: Boolean, chooseIdentity: (Boolean) -> Unit, destination: String, networked: Boolean, needsUsername: Boolean, usernameClaim: EditorUsernameClaim?, usernameControl: @Composable () -> Unit, onDismiss: () -> Unit, onPublish: suspend (String, String, String, Boolean) -> Unit) {
    var submitting by retain(draft) { mutableStateOf(false) }
    var publishError by remember { mutableStateOf<String?>(null) }
    val scope = rememberCoroutineScope()
    val dirty = draft.dirty
    val canPublish = draft.complete && !submitting && (anonymous || !needsUsername)
    val publishLabel = if (submitting) "Saving…" else if (networked) "Post to community" else "Post to nearby"
    fun publish() {
        if (!draft.complete || submitting || (!anonymous && needsUsername)) return
        submitting = true
        scope.launch {
            try {
                onPublish(draft.headline, draft.body, draft.category, anonymous)
            } catch (failure: CancellationException) {
                throw failure
            } catch (failure: Exception) {
                publishError = failure.message ?: "Please try again."
            } finally {
                submitting = false
            }
        }
    }

    val presenter = LocalPostSheetPresenter.current
    if (presenter != null) {
        presenter(PostForm(draft, destination, namedIdentity, anonymous, networked, usernameClaim, submitting,
            canPublish, publishLabel, publishError, chooseIdentity, { publishError = null }, onDismiss, ::publish))
        return
    }
    var discard by remember { mutableStateOf(false) }
    fun close() {
        if (submitting) return
        if (dirty) discard = true else onDismiss()
    }
    val form: @Composable () -> Unit = {
        val android = LocalMaterialNavigation.current != null
        Column(Modifier.fillMaxSize().imePadding().padding(top = 12.dp)) {
            Row(Modifier.fillMaxWidth().padding(horizontal = 22.dp), verticalAlignment = Alignment.CenterVertically) {
                if (android) Text("New post", Modifier.weight(1f), style = MaterialTheme.typography.headlineMedium)
                YapCancelButton(enabled = !submitting, onClick = ::close)
                if (!android) {
                    Spacer(Modifier.weight(1f))
                    YapButton(enabled = canPublish, onClick = ::publish) { Text(publishLabel) }
                }
            }
            Row(Modifier.fillMaxWidth().padding(horizontal = 22.dp), verticalAlignment = Alignment.CenterVertically) {
                IdentityPicker("Post", namedIdentity, anonymous, networked, !submitting, chooseIdentity, Modifier.weight(1f))
                if (!anonymous && needsUsername) usernameControl()
            }
            LazyColumn(Modifier.weight(1f), contentPadding = PaddingValues(22.dp), verticalArrangement = Arrangement.spacedBy(14.dp)) {
                if (!android) item { Text("New post", style = MaterialTheme.typography.headlineMedium, fontWeight = FontWeight.Bold) }
                item { Text("Posting to $destination", color = MaterialTheme.colorScheme.onSurfaceVariant) }
                item { OutlinedTextField(value = draft.headline, textStyle = postTitleStyle, enabled = !submitting, onValueChange = draft::editHeadline,
                    label = { Text("Headline") }, placeholder = { Text("Give your thought a little title") },
                    supportingText = { Text("${draft.headline.length} / $HEADLINE_LIMIT") }, modifier = Modifier.fillMaxWidth(), maxLines = 3) }
                item { OutlinedTextField(value = draft.body, textStyle = postBodyStyle, enabled = !submitting, onValueChange = draft::editBody,
                    label = { Text("Body") }, modifier = Modifier.fillMaxWidth().heightIn(min = 160.dp), placeholder = { Text("What's on your mind, neighbor?") },
                    supportingText = { Text("${draft.body.length} / $POST_LIMIT") }, minLines = 5) }
                item { Text("Pick a topic", style = MaterialTheme.typography.titleSmall) }
                item { FlowRow(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                    draft.topicChoices.forEach { value -> YapFilterChip(enabled = !submitting, selected = draft.category == value, onClick = { draft.selectTopic(value) }, label = { Text(value) }) }
                } }
            }
            if (android) Surface(color = MaterialTheme.colorScheme.surfaceContainerHigh) {
                YapButton(enabled = canPublish, onClick = ::publish,
                    modifier = Modifier.fillMaxWidth().padding(horizontal = 22.dp, vertical = 8.dp).heightIn(min = 48.dp)) {
                    ActionIcon("edit")
                    Text(publishLabel)
                }
            }
        }
        publishError?.let { message ->
            AlertDialog(onDismissRequest = { publishError = null }, title = { Text("Couldn't save that") },
                text = { Text(message) }, confirmButton = { TextButton(onClick = { publishError = null }) { Text("OK") } })
        }
    }
    val sheetState = rememberEditorSheetState { next ->
        if (next == SheetValue.Hidden && dirty && !submitting) discard = true
        next != SheetValue.Hidden || (!dirty && !submitting)
    }
    EditorSheet(onDismissRequest = ::close, sheetState = sheetState) { form() }
    if (discard) AlertDialog(onDismissRequest = { discard = false }, title = { Text("Discard your draft?") },
        text = { Text("Your words won't be saved.") },
        confirmButton = { TextButton(onClick = onDismiss) { Text("Discard") } },
        dismissButton = { TextButton(onClick = { discard = false }) { Text("Keep writing") } })
}

@Composable
private fun Profile(state: FeedSnapshot, live: Boolean, connection: NetkitState, networking: NetkitController?,
                    recoveryFiles: RecoveryFiles?, listState: LazyListState, settingsOnly: Boolean, onSettings: (() -> Unit)?, onEdit: () -> Unit, onProfile: suspend (String, CommunityProfile) -> Unit,
                    onPrivacy: () -> Unit, onGuide: () -> Unit, onCompose: () -> Unit, onReset: () -> Unit, onPost: (Long) -> Unit,
                    onJoin: suspend (String) -> Unit, onDemo: () -> Unit, onSync: () -> Unit, onDiscard: () -> Unit, onError: (String) -> Unit) {
    val native = LocalFeedControl.current
    val android = LocalMaterialNavigation.current != null
    val nativeForm = LocalNativeForm.current
    var showAllPosts by rememberSaveable { mutableStateOf(false) }
    var showRecovery by rememberSaveable { mutableStateOf(false) }
    fun androidx.compose.foundation.lazy.LazyListScope.ownPosts() {
        item {
            HorizontalDivider()
            Row(Modifier.fillMaxWidth().padding(top = 12.dp), verticalAlignment = Alignment.CenterVertically) {
                Text("Your posts", Modifier.weight(1f), style = MaterialTheme.typography.titleLarge, fontWeight = FontWeight.Bold)
                if (android) TextButton(onClick = onCompose, enabled = !connection.awaitingDefault) { Text("Write a post") }
            }
        }
        val mine = state.posts.filter { it.mine && !it.hidden }
        if (mine.isEmpty()) item { Text("No posts yet.", color = MaterialTheme.colorScheme.onSurfaceVariant) }
        fun postRows(posts: List<Post>) {
            items(posts, key = { it.id }) { post ->
                if (native != null) native(FormRow("own-post", post.headline, "story", detail = post.body,
                    onTap = { onPost(post.id) }), Modifier.fillMaxWidth().padding(vertical = 12.dp))
                else Column(Modifier.fillMaxWidth().clickable { onPost(post.id) }.padding(vertical = 12.dp)) {
                    if (post.headline.isNotBlank()) Text(post.headline, style = postTitleStyle)
                    Text(post.body, Modifier.padding(top = 6.dp), style = postBodyStyle)
                }
            }
        }
        val previewSize = 3
        postRows(if (native == null) mine else mine.take(previewSize))
        if (native != null && mine.size > previewSize) item(key = "own-posts-disclosure") {
            native(FormRow("own-posts-disclosure", if (showAllPosts) "Show fewer posts" else "Show all ${mine.size} posts",
                symbol = if (showAllPosts) "chevron.up" else "chevron.down", onTap = { showAllPosts = !showAllPosts }), Modifier)
        }
        if (native != null && showAllPosts) postRows(mine.drop(previewSize))
    }
    fun androidx.compose.foundation.lazy.LazyListScope.publicProfile() {
        item(key = "profile-identity") {
            val identity: @Composable ColumnScope.() -> Unit = {
                Text(state.alias, style = if (native != null) MaterialTheme.typography.headlineMedium else MaterialTheme.typography.headlineSmall,
                    fontWeight = FontWeight.Bold)
                if (live) UsernameControl(connection, networking)
                if (state.profile.pronouns.isNotBlank()) Text(state.profile.pronouns, color = MaterialTheme.colorScheme.primary)
                if (native == null && state.profile.identity.isNotBlank()) Text(state.profile.identity, color = MaterialTheme.colorScheme.onSurfaceVariant)
            }
            if (native != null && LocalDensity.current.fontScale >= 1.6f) {
                Column(verticalArrangement = Arrangement.spacedBy(10.dp)) {
                    ProfilePortrait(state.alias, state.profile)
                    Column(verticalArrangement = Arrangement.spacedBy(3.dp), content = identity)
                }
            } else Row(verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(16.dp)) {
                ProfilePortrait(state.alias, state.profile)
                Column(Modifier.weight(1f), verticalArrangement = Arrangement.spacedBy(if (native != null) 3.dp else 0.dp), content = identity)
            }
        }
        if (native == null) {
            if (state.profile.bio.isNotBlank()) item { Text(state.profile.bio,
                style = MaterialTheme.typography.bodyLarge, color = MaterialTheme.colorScheme.onSurfaceVariant) }
            if (state.profile.hereFor.isNotEmpty()) item { ProfileIntentions(state.profile.hereFor) }
            if (state.profile.interests.isNotBlank()) item { Text(state.profile.interests, color = MaterialTheme.colorScheme.onSurfaceVariant) }
        }
        item {
            FlowRow(Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.spacedBy(8.dp),
                verticalArrangement = Arrangement.spacedBy(4.dp)) {
                if (native != null) {
                    native(FormRow("edit", "Edit profile", symbol = "pencil", onTap = onEdit), Modifier)
                } else if (LocalMaterialNavigation.current != null) {
                    YapButton(onClick = onEdit) { ActionIcon("edit"); Text("Edit profile") }
                } else {
                    OutlinedButton(onClick = onEdit) { Text("Edit profile") }
                    OutlinedButton(onClick = onPrivacy) { Text("Privacy") }
                    if (live) TextButton(onClick = onGuide) { Text("Welcome guide") }
                }
            }
        }
        if (onSettings != null && nativeForm != null) item(key = "open-profile-settings") {
            nativeForm(NativeFormModel(sections = listOf(FormSection(rows = listOf(
                FormRow("settings", "Settings", "disclosure", symbol = "gearshape", identifier = "profile-settings", onTap = onSettings)
            )))), "inline", Modifier.fillMaxWidth())
        }
        if (native != null) {
            if (state.profile.bio.isNotBlank()) item { Text(state.profile.bio, style = postBodyStyle) }
            val details = listOf("Identity" to state.profile.identity, "Here for" to state.profile.hereFor.distinct().joinToString(", "),
                "Current joys" to state.profile.interests).filter { it.second.isNotBlank() }
            if (details.isNotEmpty()) item {
                Column(verticalArrangement = Arrangement.spacedBy(10.dp)) {
                    details.forEach { (label, value) ->
                        Column(verticalArrangement = Arrangement.spacedBy(3.dp)) {
                            Text(label, style = MaterialTheme.typography.labelMedium, color = MaterialTheme.colorScheme.onSurfaceVariant)
                            Text(value, style = MaterialTheme.typography.bodyLarge)
                        }
                    }
                }
            }
            ownPosts()
        }
    }
    LazyColumn(state = listState, contentPadding = PaddingValues(22.dp), verticalArrangement = Arrangement.spacedBy(18.dp)) {
        if (!settingsOnly) publicProfile()
        if (native != null && !settingsOnly) return@LazyColumn
        if (nativeForm != null) item(key = "profile-privacy") {
            nativeForm(NativeFormModel(sections = listOf(FormSection(rows = listOf(
                FormRow("privacy", "Privacy", "disclosure", symbol = "hand.raised", onTap = onPrivacy)
            )))), "inline", Modifier.fillMaxWidth())
        }
        if (android) {
            ownPosts()
            item {
                ProfileSection("Settings") {
                    ProfileOption("Privacy settings", "privacy", onClick = onPrivacy)
                    if (networking != null) NetworkSettings(connection, onJoin, onDemo, onDiscard, onError,
                        onJoinDefault = if (networking.offersDefaultHome) ({ networking.joinDefaultHome(state.alias); onSync() }) else null)
                    if (networking != null && recoveryFiles != null) ProfileOption("Account recovery", "key",
                        expanded = showRecovery, onClick = { showRecovery = !showRecovery })
                }
            }
        } else if (native == null) item { PolicyLinks() }
        if (!android && networking != null) item { NetworkSettings(connection, onJoin, onDemo, onDiscard, onError,
            onJoinDefault = if (networking.offersDefaultHome) ({ networking.joinDefaultHome(state.alias); onSync() }) else null) }
        if (networking != null && recoveryFiles != null && (!android || showRecovery)) item {
            DataOwnershipSection(networking, connection, recoveryFiles, state.alias, state.profile,
                onRestore = { local ->
                    val client = local["client"] as? JsonObject
                    val restoredAlias = client?.get("alias")?.jsonPrimitive?.content ?: state.alias
                    val restoredProfile = client?.get("profile")?.let { value ->
                        runCatching { kotlinx.serialization.json.Json.decodeFromJsonElement(CommunityProfile.serializer(), value) }.getOrElse {
                            error("History was recovered, but profile data could not be applied. The original data is retained in your private recovery state.")
                        }
                    } ?: state.profile
                    onProfile(restoredAlias, restoredProfile)
                }, onError = onError)
        }
        if (android) item {
            val uri = androidx.compose.ui.platform.LocalUriHandler.current
            ProfileSection("Help & about") {
                if (live) ProfileOption("Welcome guide", "guide", onClick = onGuide)
                ProfileOption("Terms of Service", "document", external = true, onClick = { uri.openUri(TERMS_URL) })
                ProfileOption("Privacy Policy", "privacy", external = true, onClick = { uri.openUri(PRIVACY_URL) })
            }
        } else if (nativeForm != null) item(key = "profile-help") {
            val uri = androidx.compose.ui.platform.LocalUriHandler.current
            val help = buildList {
                if (live) add(FormRow("guide", "Welcome guide", "disclosure", symbol = "book", onTap = onGuide))
                add(FormRow("terms", "Terms of Service", symbol = "doc.text") { uri.openUri(TERMS_URL) })
                add(FormRow("privacy", "Privacy Policy", symbol = "hand.raised") { uri.openUri(PRIVACY_URL) })
            }
            val sections = buildList {
                add(FormSection("Help & about", help))
                if (!live) add(FormSection("Local demo", listOf(FormRow("reset", "Reset demo data", destructive = true, onTap = onReset))))
            }
            nativeForm(NativeFormModel(sections = sections), "inline", Modifier.fillMaxWidth())
        } else ownPosts()
        if (!live && native == null) item {
            TextButton(onClick = onReset) { Text("Reset demo data") }
        }
    }
}

@Composable
private fun NetworkSettings(connection: NetkitState, onJoin: suspend (String) -> Unit, onDemo: () -> Unit,
                            onDiscard: () -> Unit, onError: (String) -> Unit, onJoinDefault: (suspend () -> Unit)?) {
    var invitation by remember { mutableStateOf("") }
    var joining by remember { mutableStateOf(false) }
    var editing by remember { mutableStateOf(false) }
    var discard by remember { mutableStateOf(false) }
    var expanded by remember { mutableStateOf(false) }
    val scope = rememberCoroutineScope()
    var invitationError by remember { mutableStateOf<String?>(null) }
    fun closeInvitation() {
        if (joining) return
        editing = false
        invitation = ""
        invitationError = null
    }
    fun joinDefault() {
        if (joining || onJoinDefault == null) return
        joining = true
        scope.launch {
            try { onJoinDefault() }
            catch (failure: Exception) {
                if (failure is CancellationException) throw failure
                onError(failure.message ?: "Couldn’t join yap.lgbt.")
            } finally { joining = false }
        }
    }
    val native = LocalNativeForm.current
    fun join() {
        if (joining || invitation.isBlank()) return
        joining = true
        invitationError = null
        scope.launch {
            try {
                onJoin(invitation)
                invitation = ""
                editing = false
            } catch (failure: Exception) {
                if (failure is CancellationException) throw failure
                val message = failure.message ?: "Could not join. Check the invitation."
                if (native == null) onError(message) else invitationError = message
            } finally { joining = false }
        }
    }
    if (native != null) {
        val rows = buildList {
            if (connection.enabled) add(FormRow("expand-community", if (expanded) "Hide community settings" else "Community settings") { expanded = !expanded })
            if (!connection.enabled || expanded) {
                if (connection.enabled) add(FormRow("home", connection.homeLabel ?: "Your community", "text"))
                if (!connection.enabled && !connection.awaitingDefault && onJoinDefault != null) {
                    add(FormRow("default-home", if (joining) "Joining…" else "Join yap.lgbt", enabled = !joining, onTap = ::joinDefault))
                }
                add(FormRow("invitation", "Join community", enabled = !connection.busy && !joining) { editing = true })
                if (connection.enabled || connection.awaitingDefault) add(FormRow("use-demo", "Use local demo", onTap = onDemo))
                if (connection.pendingCount > 0) add(FormRow("discard-pending", "Discard ${connection.pendingCount} pending upload(s)",
                    enabled = !connection.busy, destructive = true) { discard = true })
            }
        }
        val confirmation = if (discard) FormConfirmation("Discard pending uploads?",
            "Unsent posts, replies, and votes will not be uploaded. Already accepted posts remain on the network.",
            FormChoice("Discard uploads", destructive = true) { discard = false; onDiscard() }, FormChoice("Keep uploads") { discard = false }) else null
        native(NativeFormModel(sections = listOf(FormSection("Community", rows)), confirmation = confirmation), "inline", Modifier.fillMaxWidth())
        if (editing) native(NativeFormModel(title = "Join your community", sections = listOf(FormSection(rows = buildList {
            add(FormRow("invitation", "Private community invitation", "secure", fieldValue = { invitation }, limit = 8192,
                enabled = !joining, onChange = { if (it.length <= 8192) { invitation = it; invitationError = null } }))
            invitationError?.let { add(FormRow("error", it, "text")) }
        }, footer = "Paste the invitation your community sent you. Keep this invitation private.")),
            leading = FormChoice("Cancel", action = ::closeInvitation), leadingEnabled = !joining, trailing = FormChoice(if (joining) "Joining…" else "Join instance", action = ::join),
            trailingEnabled = invitation.isNotBlank() && !joining, canDismiss = !joining, onDismiss = ::closeInvitation),
            "sheet", Modifier)
        return
    }
    val android = LocalMaterialNavigation.current != null
    Column(verticalArrangement = Arrangement.spacedBy(if (android) 4.dp else 12.dp)) {
        if (!android) HorizontalDivider()
        if (connection.enabled) {
            if (android) ProfileOption("Community settings", "groups", expanded = expanded, onClick = { expanded = !expanded })
            else TextButton(onClick = { expanded = !expanded }) { Text(if (expanded) "Hide community settings" else "Community settings") }
            if (!expanded) return@Column
            Text(connection.homeLabel ?: "Your community", style = MaterialTheme.typography.titleMedium)
        }
        FlowRow(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
            if (!connection.enabled && !connection.awaitingDefault && onJoinDefault != null) YapButton(enabled = !joining, onClick = ::joinDefault) { Text(if (joining) "Joining…" else "Join yap.lgbt") }
            OutlinedButton(enabled = !connection.busy && !joining, onClick = { editing = true }) { Text("Join community") }
        }
        if (connection.enabled || connection.awaitingDefault) TextButton(onClick = onDemo) { Text("Use local demo") }
        if (connection.pendingCount > 0) TextButton(enabled = !connection.busy, onClick = { discard = true }) {
            Text("Discard ${connection.pendingCount} pending upload(s)")
        }
    }
    if (editing) ModalBottomSheet(onDismissRequest = { if (!joining) { editing = false; invitation = "" } },
        sheetState = rememberModalBottomSheetState(skipPartiallyExpanded = true)) {
        Column(Modifier.fillMaxWidth().fillMaxHeight(0.9f).imePadding()) {
            Row(Modifier.fillMaxWidth().padding(horizontal = 22.dp), verticalAlignment = Alignment.CenterVertically) {
                YapCancelButton(enabled = !joining, onClick = { editing = false; invitation = "" })
                Spacer(Modifier.weight(1f))
                YapButton(enabled = invitation.isNotBlank() && !joining, onClick = ::join) { Text(if (joining) "Joining…" else "Join instance") }
            }
            LazyColumn(contentPadding = PaddingValues(22.dp), verticalArrangement = Arrangement.spacedBy(18.dp)) {
                item { Text("Join your community", style = MaterialTheme.typography.headlineMedium, fontWeight = FontWeight.Bold) }
                item { Text("Paste the invitation your community sent you.", color = MaterialTheme.colorScheme.onSurfaceVariant) }
                item { OutlinedTextField(value = invitation, onValueChange = { if (it.length <= 8192) invitation = it },
                    label = { Text("Private community invitation") }, modifier = Modifier.fillMaxWidth(), minLines = 6, maxLines = 10,
                    visualTransformation = PasswordVisualTransformation(),
                    supportingText = { Text("Keep this invitation private.") }) }
            }
        }
    }
    if (discard) AlertDialog(onDismissRequest = { discard = false }, title = { Text("Discard pending uploads?") },
        text = { Text("Unsent posts, replies, and votes will not be uploaded. Already accepted posts remain on the network.") },
        confirmButton = { TextButton(onClick = { discard = false; onDiscard() }) { Text("Discard uploads") } },
        dismissButton = { TextButton(onClick = { discard = false }) { Text("Keep uploads") } })
}

@Composable
private fun EmptyState(title: String, detail: String, action: String, icon: String = "filter", onAction: () -> Unit) {
    Column(Modifier.padding(horizontal = 28.dp, vertical = 40.dp), verticalArrangement = Arrangement.spacedBy(14.dp)) {
        if (LocalMaterialNavigation.current != null) EmptyStateEmblem(icon)
        Text(title, style = MaterialTheme.typography.headlineMedium, fontWeight = FontWeight.Bold)
        Text(detail, color = MaterialTheme.colorScheme.onSurfaceVariant)
        val native = LocalFeedControl.current
        if (native != null) native(FormRow("empty", action, onTap = onAction), Modifier)
        else YapButton(onClick = onAction) { Text(action) }
    }
}

@Composable
internal expect fun AppBackHandler(enabled: Boolean, onBack: () -> Unit)
