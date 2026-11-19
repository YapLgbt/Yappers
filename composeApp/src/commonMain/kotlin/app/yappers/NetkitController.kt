package app.yappers

import kotlinx.coroutines.CancellationException
import kotlinx.coroutines.currentCoroutineContext
import kotlinx.coroutines.delay
import kotlinx.coroutines.isActive
import kotlinx.coroutines.withTimeoutOrNull
import kotlinx.coroutines.flow.first
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.NonCancellable
import kotlinx.coroutines.sync.Mutex
import kotlinx.coroutines.sync.withLock
import kotlinx.coroutines.withContext
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.flow.update
import kotlinx.serialization.Serializable
import kotlinx.serialization.encodeToString
import kotlinx.serialization.json.*

private const val IDENTITY_KEY = "identity-v1"
private const val PUBLIC_ROUTE_KEY = "public-feed-route-v1"
private const val CONNECTION_KEY = "connection-v1"
private const val GROUPS_KEY = "groups-v1"
private const val MISSING_VALUE = "null"
private const val WELCOME_STAGE_KEY = "welcome-backup-v1"
private const val PROFILE_STAGE_PREFIX = "profile:"
private const val MAX_OUTBOX_SIZE = 100
private const val MAX_UPLOADS_PER_SYNC = 20

@Serializable
private data class WelcomeProfile(val name: String, val profile: CommunityProfile)

@Serializable
private data class Identity(val secret: String, val nextSequence: Long = 1, val wallet: JsonObject? = null)

@Serializable
private data class PublicRoute(val target: String, val replica: JsonObject? = null, val fullHomeStandby: JsonObject? = null) {
    fun standbyFor(join: JsonObject): JsonObject? = fullHomeStandby?.takeIf {
        join["serverAddr"]?.jsonObject?.get("id")?.jsonPrimitive?.content == target.substringBefore('/')
    }
}

@Serializable
private data class ConnectionPreference(val enabled: Boolean, val join: JsonObject, val homeLabel: String? = null)

/**
 * Serializes connection ownership and durable mutations behind one mutex.
 *
 * Crash ordering: reserve author sequence → persist signed outbox event → acknowledge
 * locally → submit → persist receipt removal. Sequence gaps are safe; reuse is not.
 * Never regenerate an event after an uncertain network result. See docs/netkit/ios-ffi.md.
 *
 * Construct once per app controller. [setForeground] can interrupt a sync between calls;
 * [shutdown] releases transport resources while keeping the saved live-mode preference.
 */
class NetkitController(private val storage: NetkitStorage, private val engine: NetkitEngine,
                       private val defaultHome: (suspend () -> String)? = null) {
    private val json = Json { encodeDefaults = true }
    private val mutex = Mutex()
    private val foreground = MutableStateFlow(true)
    private var handle: Long? = null
    private var publicRoute: PublicRoute? = null
    private var lastDiscovery: kotlin.time.TimeMark? = null
    private var activeDevice: String? = null
    private var identity: Identity? = null
    private var loadError: String? = null
    private var archives = emptyMap<String, NetkitCache>()
    private var policies = emptyMap<String, DevicePrivacy>()
    private var privacyScope: String? = null
    private val policy get() = policies[privacyScope] ?: DevicePrivacy()
    private var checkMailboxRegistration = true
    private var history = emptyList<DirectMessage>()
    private var waiting = emptyList<String>()
    private var cache = loadCache()
    private val privateMessages = PrivateMessages(storage, engine)
    private var groupsLoadError: String? = null
    private var groupPreferences = loadGroupPreferences()

    private fun loadGroupPreferences(): GroupPreferences = try {
        storage.readSecret(GROUPS_KEY).takeUnless { it == MISSING_VALUE }?.let {
            json.decodeFromString<GroupPreferences>(it).also { prefs -> require(prefs.version == 1) }
        } ?: GroupPreferences()
    } catch (_: Exception) {
        groupsLoadError = "Saved groups could not be read. Unlock your device before changing them."
        GroupPreferences()
    }


    /** Accept the original single-cache file, then write the archive wrapper on the next commit. */
    private fun loadCache(): NetkitCache = try {
        val encoded = storage.readState().takeUnless { it == MISSING_VALUE }
        if (encoded == null) NetkitCache(snapshot = FeedSnapshot(emptyList(), alias = suggestName())) else {
            val root = json.parseToJsonElement(encoded).jsonObject
            if ("current" in root) {
                val disk = json.decodeFromString<NetkitDiskState>(encoded)
                require(disk.version == 1 && disk.current.version == 1 &&
                    disk.communities.values.all { it.version == 1 && it.outbox.isEmpty() }) { "Unsupported network cache version" }
                policies = disk.privacy
                archives = disk.communities
                disk.current
            } else {
                json.decodeFromString<NetkitCache>(encoded).also {
                    require(it.version == 1) { "Unsupported network cache version" }
                }
            }
        }
    } catch (_: Exception) {
        // Recovering with an empty outbox would silently lose undelivered publications.
        loadError = "Network cache could not be read. Restore it before reconnecting."
        NetkitCache()
    }
    private fun retainedDrafts(current: NetkitCache): List<JsonObject> =
        (archives.values + current).flatMap { saved ->
            (saved.recoveryLocalData?.get("recoveredPendingEvents") as? JsonArray).orEmpty()
                .filterIsInstance<JsonObject>()
        }.distinctBy { it["id"] }

    private val mutableState = MutableStateFlow(NetkitState(
        awaitingDefault = defaultHome != null,
        snapshot = cache.snapshot.copy(posts = emptyList()),
        pendingCount = cache.outbox.size,
        ownedHistory = (archives.values + cache).flatMap { it.ownedEvents }.distinctBy { (it["event"] as? JsonObject)?.get("id") ?: it },
        ownershipReady = runCatching { storage.readSecret(IDENTITY_KEY).takeUnless { it == MISSING_VALUE }?.let { json.decodeFromString<Identity>(it).wallet != null } == true }.getOrDefault(false),
        recoveredDrafts = retainedDrafts(cache),
        status = loadError ?: "Local demo",
        groups = groupPreferences.joined.values.map { it.summary },
        joinedGroups = groupPreferences.joined.values.map { it.summary },
        locality = groupPreferences.locality,
        hasHomeFeed = groupPreferences.home != null,
        groupsStatus = groupsLoadError.orEmpty()
    ))
    val state: StateFlow<NetkitState> = mutableState.asStateFlow()

    /** Stop new RPCs between operations without waiting for the connection mutex. */
    fun setForeground(active: Boolean) {
        foreground.value = active
    }

    private suspend fun commit(candidate: NetkitCache, otherCaches: Map<String, NetkitCache> = archives, privacy: Map<String, DevicePrivacy> = policies, switching: Boolean = false) {
        val known = candidate.deleted + cache.deleted + archives.values.flatMap { it.deleted }
        val next = candidate.purgeDeleted(known)
        val nextArchives = otherCaches.toMutableMap()
        if (cache.target != next.target) cache.target?.let { nextArchives[it] = cache }
        next.target?.let { nextArchives.remove(it) }
        if (known.isNotEmpty()) nextArchives.keys.toList().forEach { target ->
            nextArchives[target] = nextArchives.getValue(target).purgeDeleted(known)
        }
        // Once the atomic file is written, publish the matching memory state even if cancelled.
        withContext(NonCancellable + Dispatchers.Default) {
            storage.writeState(json.encodeToString(NetkitDiskState(current = next, communities = nextArchives, privacy = privacy)))
            cache = next
            archives = nextArchives
            policies = privacy
            mutableState.update { it.copy(snapshot = if (switching) next.snapshot.copy(posts = emptyList()) else policy.filter(next.snapshot), pendingCount = next.outbox.size,
                ownedHistory = (archives.values + next).flatMap { it.ownedEvents }.distinctBy { (it["event"] as? JsonObject)?.get("id") ?: it },
                recoveredDrafts = retainedDrafts(next)) }
        }
    }
    private suspend fun secret(key: String): String? = withContext(Dispatchers.Default) {
        storage.readSecret(key).takeUnless { it == MISSING_VALUE }
    }

    private suspend fun connectionPreference(): ConnectionPreference? = secret(CONNECTION_KEY)?.let {
        json.decodeFromString<ConnectionPreference>(it)
    }

    private suspend fun loadIdentity(): Identity {
        val stored = secret(IDENTITY_KEY)
        if (stored != null) {
            return try {
                json.decodeFromString<Identity>(stored)
            } catch (_: Exception) {
                error("Saved identity is unreadable. Restore it before reconnecting.")
            }
        }
        val created = Identity(engine.identity().getValue("secret").jsonPrimitive.content)
        writeSecret(IDENTITY_KEY, json.encodeToString(created))
        return created
    }

    private suspend fun installPublicRoute(enrollment: PublicEnrollment) {
        val join = enrollment.join
        val target = join.getValue("serverAddr").jsonObject.getValue("id").jsonPrimitive.content + "/" + join.getValue("community").jsonPrimitive.content
        val activeJoin = if (target == cache.target) join else {
            if (publicRoute?.target != target || cache.target?.substringBefore('/') != target.substringBefore('/')) return
            connectionPreference()?.join ?: return
        }
        val next = PublicRoute(target, enrollment.replica, enrollment.fullHomeStandby)
        if (next != publicRoute) {
            val active = handle
            val replacement = if (active != null && publicRoute?.standbyFor(join) != next.fullHomeStandby) {
                openClient(activeJoin, checkNotNull(identity), next)
            } else null
            withContext(NonCancellable) {
                try {
                    writeSecret(PUBLIC_ROUTE_KEY, json.encodeToString(next))
                } catch (failure: Exception) {
                    replacement?.let { engine.close(it.getValue("handle").jsonPrimitive.long) }
                    throw failure
                }
                publicRoute = next
                if (replacement != null) {
                    handle = replacement.getValue("handle").jsonPrimitive.long
                    engine.close(checkNotNull(active))
                }
            }
        }
        lastDiscovery = kotlin.time.TimeSource.Monotonic.markNow()
    }

    private suspend fun refreshPublicRoute() {
        val discover = defaultHome ?: return
        if (state.value.homeLabel != AppBrand.domain &&
            (publicRoute == null || publicRoute?.target?.substringBefore('/') != cache.target?.substringBefore('/'))) return
        if (lastDiscovery?.elapsedNow()?.inWholeSeconds?.let { it < 300 } == true) return
        lastDiscovery = kotlin.time.TimeSource.Monotonic.markNow()
        try {
            withTimeoutOrNull(4000) { publicEnrollment(discover()) }?.let { installPublicRoute(it) }
        } catch (failure: Exception) {
            if (failure is CancellationException) throw failure
            // Keep the pinned route if HTTPS is temporarily unreachable.
        }
    }

    private val mutableWelcomeUsername = MutableStateFlow(WelcomeUsername.Unchecked)
    internal val welcomeUsername = mutableWelcomeUsername.asStateFlow()
    internal var finishingRestore = false
        private set

    internal suspend fun checkWelcomeUsername(retry: Boolean = false) = mutex.withLock {
        if (!retry && mutableWelcomeUsername.value != WelcomeUsername.Unchecked) return@withLock
        val active = checkNotNull(handle) { "Reconnect to your community to verify your username." }
        val feed = engine.request(active, buildJsonObject { put("type", "feed") })
        val result = if (feed["privacy"]?.jsonPrimitive?.booleanOrNull == true) {
            val wallet = checkNotNull(identity?.wallet) { "Reconnect to your account to verify your username." }
            engine.request(active, buildJsonObject { put("type", "accountRegister"); put("wallet", wallet) })
            social("privacyStatus")
        } else null
        val domain = result?.get("usernameDomain")?.jsonPrimitive?.contentOrNull
        if (domain.isNullOrBlank()) {
            mutableWelcomeUsername.value = WelcomeUsername.Unsupported
            return@withLock
        }
        val username = result["username"]?.jsonPrimitive?.contentOrNull
        check(username == null || username.substringAfter('@', "") == domain && validUsername(username.substringBefore('@'))) {
            "Your community returned an invalid username. Try again."
        }
        savePolicy(policy.copy(usernameDomain = domain, username = username))
        mutableWelcomeUsername.value = if (username == null) WelcomeUsername.Required else WelcomeUsername.Confirmed
    }

    private val mutableBackupStep = MutableStateFlow(false)
    internal val backupStep = mutableBackupStep.asStateFlow()

    internal suspend fun enterNeighborhood() {
        check(!mutableBackupStep.value || mutableWelcomeUsername.value == WelcomeUsername.Confirmed) { "Claim a username before entering the neighborhood." }
        writeSecret(WELCOME_STAGE_KEY, MISSING_VALUE)
        finishingRestore = false
        mutableBackupStep.value = false
    }

    private val mutableWelcome = MutableStateFlow(false)
    internal val welcome: StateFlow<Boolean> = mutableWelcome.asStateFlow()

    /** Called once by the platform before starting its foreground poller. */
    internal fun prepareWelcome(hasLocalProfile: Boolean) {
        val stage = runCatching { storage.readSecret(WELCOME_STAGE_KEY) }.getOrNull()
        finishingRestore = stage == "restore"
        mutableBackupStep.value = stage == "pending" || finishingRestore || stage?.startsWith(PROFILE_STAGE_PREFIX) == true
        mutableWelcome.value = !hasLocalProfile && runCatching {
            storage.readSecret(IDENTITY_KEY) == MISSING_VALUE && storage.readSecret(CONNECTION_KEY) == MISSING_VALUE
        }.getOrDefault(false)
    }

    internal suspend fun persistWelcomeProfile(save: (String, CommunityProfile) -> Unit) = mutex.withLock {
        val stage = secret(WELCOME_STAGE_KEY) ?: return@withLock
        if (stage.startsWith(PROFILE_STAGE_PREFIX)) {
            val draft = json.decodeFromString<WelcomeProfile>(stage.removePrefix(PROFILE_STAGE_PREFIX))
            save(draft.name, draft.profile)
            // Retrying after an interruption writes the same profile before advancing the checkpoint.
            writeSecret(WELCOME_STAGE_KEY, "pending")
        }
    }

    internal suspend fun finishWelcome(alias: String, recoveryKey: String? = null, profile: CommunityProfile? = null) {
        require(alias.trim().length in 2..30) { "Choose a display name between 2 and 30 characters." }
        val enrollment = publicEnrollment(checkNotNull(defaultHome).invoke())
        mutex.withLock {
            check(mutableWelcome.value && !state.value.enabled) { "Your account is already open." }
            check(secret(CONNECTION_KEY) == null && (recoveryKey == null || secret(IDENTITY_KEY) == null)) {
                "There is already an account on this device. Reopen Yap to continue."
            }
            val join = enrollment.join
            val home = join.getValue("serverAddr").jsonObject.getValue("id").jsonPrimitive.content
            val author = if (recoveryKey == null) null else {
                val recovered = engine.portability(buildJsonObject {
                    put("type", "readRecoveryKey"); put("input", recoveryKey.trim()); put("home", home)
                })
                val account = recovered.getValue("account")
                val probe = engine.open(buildJsonObject {
                    put("join", join); put("authorSecret", engine.identity().getValue("secret")); put("nextSequence", 1)
                    enrollment.fullHomeStandby?.let { put("fullHomeStandby", it) }
                }).getValue("handle").jsonPrimitive.long
                try {
                    val chain = engine.request(probe, buildJsonObject {
                        put("type", "accountLookup"); put("account", account)
                    }).getValue("chain").jsonArray
                    check(chain.isNotEmpty() && chain.last().jsonObject.getValue("payload").jsonObject["home"]?.jsonPrimitive?.content == home) {
                        "Your home could not confirm this account."
                    }
                    val pending = secret("welcome-device-v1")?.let { json.parseToJsonElement(it).jsonObject }
                    val prior = pending?.get("chain")?.jsonArray
                    val wallet = if (pending != null && prior != null && (prior == chain || prior.dropLast(1) == chain)) pending
                    else engine.portability(buildJsonObject {
                        put("type", "recover"); put("rootSecret", recovered.getValue("root")); put("chain", chain)
                        put("home", home); put("retainDevices", true)
                    }).getValue("wallet").jsonObject
                    writeSecret("welcome-device-v1", wallet.toString())
                    engine.request(probe, buildJsonObject { put("type", "accountRegister"); put("wallet", wallet) })
                    Identity(wallet.getValue("deviceSecret").jsonPrimitive.content, wallet.getValue("nextSequence").jsonPrimitive.long, wallet)
                } finally { withContext(NonCancellable) { engine.close(probe) } }
            }
            // Persist the gate before committing credentials so a relaunch resumes setup.
            val stage = when {
                author != null -> "restore"
                profile != null -> PROFILE_STAGE_PREFIX + json.encodeToString(WelcomeProfile(alias.trim(), profile.cleaned()))
                else -> "pending"
            }
            writeSecret(WELCOME_STAGE_KEY, stage)
            finishingRestore = author != null
            mutableBackupStep.value = true
            connectLocked(join.toString(), alias.trim(), author, AppBrand.domain)
            try {
                installPublicRoute(enrollment)
                writeSecret("welcome-device-v1", MISSING_VALUE)
            } finally { mutableWelcome.value = false }
        }
    }

    internal suspend fun keyFromBackup(ciphertext: String, passphrase: String): String {
        val wallet = engine.portability(buildJsonObject {
            put("type", "decrypt"); put("ciphertext", ciphertext); put("passphrase", passphrase)
        }).getValue("bundle").jsonObject.getValue("wallet")
        return engine.portability(buildJsonObject { put("type", "recoveryKey"); put("wallet", wallet) }).getValue("key").jsonPrimitive.content
    }

    internal suspend fun recoveryKey(): String = mutex.withLock {
        val wallet = checkNotNull(identity?.wallet) { "Connect to your account first." }
        engine.request(checkNotNull(handle), buildJsonObject { put("type", "accountRegister"); put("wallet", wallet) })
        engine.portability(buildJsonObject { put("type", "recoveryKey"); put("wallet", wallet) }).getValue("key").jsonPrimitive.content
    }

    /** Restore only an explicitly enabled preference; corrupted credentials never mint a replacement. */
    suspend fun restore(alias: String = cache.snapshot.alias) {
        try {
            val shouldEnroll = mutex.withLock {
                val preference = connectionPreference()
                when {
                    preference?.enabled == true -> {
                        connectLocked(preference.join.toString(), cache.snapshot.alias, homeLabel = preference.homeLabel)
                        false
                    }
                    preference != null || defaultHome == null -> {
                        mutableState.update { it.copy(awaitingDefault = false) }
                        false
                    }
                    else -> {
                        mutableState.update { it.copy(awaitingDefault = true, busy = true, status = "Joining yap.lgbt…") }
                        true
                    }
                }
            }
            if (shouldEnroll) {
                val enrollment = publicEnrollment(checkNotNull(defaultHome).invoke())
                mutex.withLock {
                    // A manual invitation or demo choice made during discovery takes precedence.
                    if (connectionPreference() == null) {
                        connectLocked(enrollment.join.toString(), alias, homeLabel = AppBrand.domain)
                        installPublicRoute(enrollment)
                    }
                }
            }
        } catch (failure: Exception) {
            if (failure is CancellationException) throw failure
            mutableState.update { it.copy(feedUnavailable = true, status = if (it.awaitingDefault)
                "Couldn’t connect to your home. Check your connection and retry."
                else "Could not restore your saved connection. Unlock your device and reopen Yap.") }
        } finally {
            mutableState.update { it.copy(busy = false) }
        }
    }

    val offersDefaultHome: Boolean get() = defaultHome != null

    suspend fun joinDefaultHome(alias: String) {
        val enrollment = publicEnrollment(checkNotNull(defaultHome).invoke())
        mutex.withLock {
            check(!state.value.enabled) { "Return to your local demo before changing homes." }
            connectLocked(enrollment.join.toString(), alias, homeLabel = AppBrand.domain)
            installPublicRoute(enrollment)
        }
    }

    private suspend fun openClient(join: JsonObject, author: Identity, route: PublicRoute?) = engine.open(buildJsonObject {
        put("join", join)
        put("authorSecret", author.secret)
        put("nextSequence", author.nextSequence)
        author.wallet?.let { put("accountChain", it.getValue("chain")) }
        route?.standbyFor(join)?.let { put("fullHomeStandby", it) }
    })

    /** Stage a validated connection; never transfer an old community's outbox to a new home. */
    suspend fun connect(joinText: String, alias: String) = mutex.withLock {
        connectLocked(joinText, alias)
    }

    // Caller holds mutex. Shared by private invitations and saved group switching.
    private suspend fun connectLocked(joinText: String, alias: String, restoredIdentity: Identity? = null, homeLabel: String? = null) {
        loadError?.let { error(it) }
        val join = try {
            json.parseToJsonElement(joinText).jsonObject
        } catch (_: Exception) {
            error("Invalid join configuration JSON. Check the private join file.")
        }
        val previousRoute = secret(PUBLIC_ROUTE_KEY)?.let { json.decodeFromString<PublicRoute>(it) }
        val freshAccount = restoredIdentity == null && secret(IDENTITY_KEY) == null
        var author = restoredIdentity ?: loadIdentity()
        if (restoredIdentity == null && engine.supportsPortability && author.wallet == null && cache.outbox.isEmpty()) {
            val home = join.getValue("serverAddr").jsonObject.getValue("id").jsonPrimitive.content
            val wallet = engine.portability(buildJsonObject {
                put("type", "create"); put("rootSecret", author.secret); put("home", home)
            }).getValue("wallet").jsonObject
            author = Identity(wallet.getValue("deviceSecret").jsonPrimitive.content, wallet.getValue("nextSequence").jsonPrimitive.long, wallet)
        }
        val opened = openClient(join, author, previousRoute)
        val nextHandle = opened.getValue("handle").jsonPrimitive.long
        var installed = false
        try {
            val target = opened.getValue("home").jsonPrimitive.content + "/" + opened.getValue("community").jsonPrimitive.content
            check(cache.outbox.isEmpty() || cache.target == target) { "Deliver or explicitly discard pending uploads before switching communities." }
            val selected = if (cache.target == target) cache else archives[target] ?: NetkitCache(target = target)
            val next = selected.copy(snapshot = selected.snapshot.copy(alias = alias))
            val previousPreference = secret(CONNECTION_KEY) ?: MISSING_VALUE
            val previousIdentity = secret(IDENTITY_KEY) ?: MISSING_VALUE
            withContext(NonCancellable) {
                writeSecret(CONNECTION_KEY, json.encodeToString(ConnectionPreference(true, join, homeLabel)))
                try {
                    writeSecret(IDENTITY_KEY, json.encodeToString(author))
                    commit(next, switching = true)
                } catch (error: Exception) {
                    writeSecret(IDENTITY_KEY, previousIdentity)
                    writeSecret(CONNECTION_KEY, previousPreference)
                    throw error
                }
                publicRoute = previousRoute
                mutableWelcomeUsername.value = WelcomeUsername.Unchecked
                val previousHandle = handle
                activeDevice = opened["author"]?.jsonPrimitive?.content
                handle = nextHandle
                identity = author
                installed = true
                val account = author.wallet?.get("chain")?.jsonArray?.lastOrNull()?.jsonObject?.get("payload")?.jsonObject?.get("account")?.jsonPrimitive?.content
                privacyScope = target.substringBefore('/') + "/" + (account ?: activeDevice ?: "legacy")
                checkMailboxRegistration = true
                history = emptyList()
                waiting = emptyList()
                if (freshAccount && privacyScope !in policies) savePolicy(policy.copy(homeAllowsPublicView = true))
                mutableState.update { it.withPrivacy(policy, cache.snapshot, history, waiting).copy(supportsPrivacy = false, enabled = true, feedUnavailable = false, defaultUsernameDomain = AppBrand.domain.takeIf { target.substringBefore('/') == "114f92b50e66dd84a14e0ff9b857b0c254565609b00b0cb9ef85f29e864efe5b" }, messagingScope = target.substringBefore('/') + activeDevice, directAvailable = false, mailboxElsewhere = false, messages = emptyList(), blockedAccounts = emptyList(), messageStatus = null, homeLabel = homeLabel, awaitingDefault = false, online = false, ownershipReady = author.wallet != null, status = "Ready to sync",
                    activeGroup = groupPreferences.joined[join["community"]?.jsonPrimitive?.content]?.summary) }
                previousHandle?.let { runCatching { engine.close(it) } }
                if (activeDevice != null && author.wallet != null) {
                    try {
                        privateMessages.read(messageKey())?.let { saved ->
                            showMessages(saved)
                        }
                    } catch (failure: Exception) {
                        mutableState.update { it.copy(messageStatus = failure.message ?: "Couldn’t open messages on this device.") }
                    }
                }
            }
        } catch (error: Exception) {
            if (!installed) withContext(NonCancellable) { engine.close(nextHandle) }
            throw error
        }
    }

    /** Disable automatic restore without deleting cached content or pending uploads. */
    suspend fun useDemo() = mutex.withLock {
        val preference = connectionPreference()
        withContext(NonCancellable) {
            writeSecret(CONNECTION_KEY, json.encodeToString(ConnectionPreference(false, preference?.join ?: buildJsonObject {})))
            closeActiveHandle()
            history = emptyList(); waiting = emptyList(); privacyScope = null
            mutableState.update { it.copy(privacy = DevicePrivacy(), supportsPrivacy = false, requests = emptyList(), enabled = false, feedUnavailable = false, messagingScope = null, directAvailable = false, mailboxElsewhere = false, messages = emptyList(), blockedAccounts = emptyList(), messageStatus = null, homeLabel = null, awaitingDefault = false, online = false, busy = false, status = "Local demo") }
        }
    }

    internal suspend fun hasOpenConnection(): Boolean = mutex.withLock { handle != null }

    internal suspend fun closeIfBackground() = mutex.withLock {
        if (!foreground.value) withContext(NonCancellable) { closeActiveHandle() }
    }

    /** Release native resources; a foreground restore can reopen the saved connection. */
    suspend fun shutdown() = mutex.withLock {
        withContext(NonCancellable) { closeActiveHandle() }
    }

    // Caller holds mutex. A close failure retains ownership so a later shutdown can retry.
    private suspend fun closeActiveHandle() {
        handle?.let { engine.close(it) }
        handle = null
        activeDevice = null
    }

    private suspend fun writeSecret(key: String, value: String) = withContext(Dispatchers.Default) {
        storage.writeSecret(key, value)
    }

    private suspend fun commitGroups(next: GroupPreferences) {
        groupsLoadError?.let { error(it) }
        withContext(NonCancellable + Dispatchers.Default) {
            storage.writeSecret(GROUPS_KEY, json.encodeToString(next))
            groupPreferences = next
            mutableState.update { it.copy(joinedGroups = next.joined.values.map { saved -> saved.summary }, locality = next.locality, hasHomeFeed = next.home != null) }
        }
    }

    /** City selection stays local; only public-group membership can reveal a city interest. */
    suspend fun selectLocality(locality: GroupLocality?) = mutex.withLock {
        commitGroups(groupPreferences.copy(locality = locality))
    }

    suspend fun refreshGroups() = mutex.withLock {
        // An activity can recreate while the previous window is closing its handle.
        val active = handle?.takeIf { state.value.enabled } ?: return@withLock
        mutableState.update { it.copy(groupsBusy = true, groupsStatus = "Finding your people…") }
        try {
            val result = engine.request(active, buildJsonObject { put("type", "groups") })
            val groups = result.getValue("groups").jsonArray.map { json.decodeFromJsonElement<GroupSummary>(it) }
            mutableState.update { it.copy(groups = groups, groupsStatus = "") }
        } catch (error: Exception) {
            if (error is CancellationException) throw error
            mutableState.update { it.copy(groupsStatus = "Couldn’t refresh groups. Try again.") }
        } finally { mutableState.update { it.copy(groupsBusy = false) } }
    }

    /** Enrollment is remote, but active feed switching still preserves the old durable outbox. */
    suspend fun joinGroup(group: GroupSummary) = mutex.withLock {
        checkGroupSwitch()
        val active = checkNotNull(handle) { "Connect a home first." }
        val result = engine.request(active, buildJsonObject { put("type", "joinGroup"); put("group", group.id) })
        installGroup(result)
    }

    suspend fun openGroup(id: String) = mutex.withLock {
        loadError?.let { error(it) }
        groupsLoadError?.let { error(it) }
        val saved = groupPreferences.joined[id] ?: error("Join this group first.")
        connectLocked(saved.join.toString(), cache.snapshot.alias)
    }

    /** Return to the private/home feed retained before group navigation. */
    suspend fun openHomeFeed() = mutex.withLock {
        loadError?.let { error(it) }
        groupsLoadError?.let { error(it) }
        val home = groupPreferences.home ?: error("Connect a home first.")
        connectLocked(home.toString(), cache.snapshot.alias)
    }

    suspend fun createGroup(name: String, description: String, category: String, locality: GroupLocality?) = mutex.withLock {
        checkGroupSwitch()
        val active = checkNotNull(handle) { "Connect a home first." }
        val result = engine.request(active, buildJsonObject {
            put("type", "createGroup")
            put("details", buildJsonObject {
                put("name", name.trim()); put("description", description.trim()); put("category", category)
                put("locality", locality?.let { json.encodeToJsonElement(it) } ?: JsonNull)
            })
        })
        installGroup(result)
    }

    private fun checkGroupSwitch() {
        loadError?.let { error(it) }
        groupsLoadError?.let { error(it) }
        check(cache.outbox.isEmpty()) { "Deliver or discard pending uploads before switching groups." }
        check(state.value.enabled) { "Connect a home first." }
    }

    // Persist returned capabilities before switching. If switching fails, the saved group can be reopened.
    private suspend fun installGroup(result: JsonObject) {
        val group = json.decodeFromJsonElement<GroupSummary>(result.getValue("group"))
        val join = result.getValue("join").jsonObject
        check(join.getValue("community").jsonPrimitive.content == group.id) { "Group invitation mismatch" }
        check(groupPreferences.joined.size < 32 || group.id in groupPreferences.joined) { "Saved group limit reached." }
        val home = if (state.value.activeGroup == null) connectionPreference()?.join else groupPreferences.home
        commitGroups(groupPreferences.copy(joined = groupPreferences.joined + (group.id to SavedGroup(group, join)),
            locality = group.locality ?: groupPreferences.locality, home = home))
        connectLocked(join.toString(), cache.snapshot.alias)
        mutableState.update { current -> current.copy(groups = current.groups.filterNot { it.id == group.id } + group) }
    }

    private suspend fun enqueue(action: JsonObject) = mutex.withLock {
        val active = checkNotNull(handle) { "Join an instance first." }
        check(state.value.enabled) { "Live mode is not enabled." }
        val named = when (action["kind"]?.jsonPrimitive?.content) {
            "post", "postV2" -> action["anonymous"]?.jsonPrimitive?.boolean == false
            "reply" -> action["displayName"]?.jsonPrimitive?.content != "Anonymous neighbor"
            else -> false
        }
        check(!named || !state.value.needsUsername) { "Claim a username before posting by name." }
        check(cache.outbox.size < MAX_OUTBOX_SIZE) { "Outbox is full. Sync or discard pending uploads first." }
        val prepared = engine.request(active, buildJsonObject { put("type", "prepare"); put("action", action) })
        val event = prepared.getValue("event").jsonObject
        val sequence = prepared.getValue("nextSequence").jsonPrimitive.long
        val previous = checkNotNull(identity)
        val next = previous.copy(nextSequence = sequence, wallet = previous.wallet?.let { wallet ->
            JsonObject(wallet + ("nextSequence" to JsonPrimitive(sequence)))
        })
        // Reserve sequence before exposing or delivering this operation. Gaps are safe after failures.
        writeSecret(IDENTITY_KEY, json.encodeToString(next))
        identity = next
        commit(cache.copy(outbox = cache.outbox + event).overlay(event))
        mutableState.update { it.copy(status = "${cache.outbox.size} pending upload(s)") }
    }

    /**
     * Encrypted user-owned recovery. Callers save ciphertext to a private file/share sheet;
     * neither passphrase nor plaintext is written to the network cache or logs.
     * localData can carry the caller's profile; group references and preferences are included.
     */
    suspend fun exportRecovery(passphrase: String, localData: JsonObject = buildJsonObject {}, refreshFromHome: Boolean = false): String = mutex.withLock {
        val savedIdentity = identity ?: secret(IDENTITY_KEY)?.let { json.decodeFromString<Identity>(it) }
        val wallet = checkNotNull(savedIdentity?.wallet) { "Reconnect with an empty outbox to enable portable identity." }
        val owned = (archives.values + cache).flatMap { it.ownedEvents }.toMutableList()
        if (refreshFromHome) {
            val active = checkNotNull(handle) { "Connect a home to refresh older authored history." }
            val confirmed = engine.request(active, buildJsonObject { put("type", "accountExport"); put("wallet", wallet) }).getValue("events").jsonArray.map { it.jsonObject }
            val erased = confirmed.filter { it["deletion"] != null }
            if (erased.isNotEmpty()) {
                val ids = erased.map { it.getValue("event").jsonObject.getValue("id").jsonPrimitive.content }.toSet()
                commit(cache.copy(ownedEvents = (erased + cache.ownedEvents).distinctBy { it.getValue("event").jsonObject.getValue("id") }).purgeDeleted(ids))
            }
            owned += confirmed
        }
        val unique = owned.sortedByDescending { it["deletion"] != null }.distinctBy { it.getValue("event").jsonObject.getValue("id") }
        val backupWallet = JsonObject(wallet + ("pending" to JsonArray(cache.outbox)))
        val sourceHomeAddress = connectionPreference()?.join?.get("serverAddr")
        val preferences = buildJsonArray {
            (archives.values + cache).filter { it.target != null }.forEach { saved ->
                add(buildJsonObject {
                    put("community", saved.target!!.substringAfter('/'))
                    put("bookmarks", buildJsonArray { saved.bookmarks.forEach { add(it) } })
                    put("hidden", buildJsonArray { saved.hidden.forEach { add(it) } })
                })
            }
        }
        engine.portability(buildJsonObject {
            put("type", "encrypt"); put("wallet", backupWallet); put("events", JsonArray(unique)); put("passphrase", passphrase)
            put("localData", buildJsonObject {
                sourceHomeAddress?.let { put("sourceHomeAddress", it) }
                put("historyCoverage", if (refreshFromHome) "local-plus-home" else "local-receipts")
                put("client", localData); put("preferences", preferences)
                val drafts = (archives.values + cache).flatMap { saved ->
                    saved.recoveryLocalData?.get("recoveredPendingEvents")?.jsonArray.orEmpty()
                }.distinctBy { it.jsonObject.getValue("id") }
                put("recoveredPendingEvents", JsonArray(drafts))
                put("joinedGroups", json.encodeToJsonElement(groupPreferences.joined.values.map { it.summary }))
                put("locality", json.encodeToJsonElement(groupPreferences.locality))
            })
        }).getValue("ciphertext").jsonPrimitive.content
    }

    /**
     * Move/recover without the old home's permission. Group admission remains independent.
     * Persist the new identity before the first upload; repeating the same import resumes
     * with that exact device/revision. Keep the encrypted source until every page succeeds.
     * Returns caller-owned profile/local data for explicit application by the UI host.
     */
    suspend fun importRecovery(ciphertext: String, passphrase: String, joinText: String, alias: String, pendingAsDrafts: Boolean = false): JsonObject = mutex.withLock {
        check(cache.outbox.isEmpty()) { "Deliver pending uploads before importing another home." }
        val bundle = engine.portability(buildJsonObject {
            put("type", "decrypt"); put("ciphertext", ciphertext); put("passphrase", passphrase)
        }).getValue("bundle").jsonObject
        val recoveryWallet = bundle.getValue("wallet").jsonObject
        val recoveredDrafts = recoveryWallet.getValue("pending").jsonArray
        check(recoveredDrafts.isEmpty() || pendingAsDrafts) { "Recovery contains uncertain pending uploads. Deliver them first, or explicitly recover them as drafts." }
        val oldWallet = JsonObject(recoveryWallet + ("pending" to buildJsonArray {}))
        val archive = bundle.getValue("archive").jsonObject.getValue("payload").jsonObject
        val account = archive.getValue("account").jsonPrimitive.content
        groupsLoadError?.let { error(it) }
        val destinationJoin = try { json.parseToJsonElement(joinText).jsonObject }
            catch (_: Exception) { error("Invalid private invitation. Check the join file.") }
        val destination = try { destinationJoin.getValue("serverAddr").jsonObject.getValue("id").jsonPrimitive.content }
            catch (_: Exception) { error("Private invitation is missing its home address.") }
        val savedIdentity = identity ?: secret(IDENTITY_KEY)?.let { json.decodeFromString<Identity>(it) }
        check(savedIdentity == null || savedIdentity.wallet != null) { "Reconnect with an empty outbox to upgrade and export this device's existing identity first." }
        val saved = savedIdentity?.wallet
        fun record(wallet: JsonObject) = wallet.getValue("chain").jsonArray.last().jsonObject.getValue("payload").jsonObject
        check(saved == null || record(saved).getValue("account").jsonPrimitive.content == account) {
            "This device holds a different account. Export it before using a separate profile for recovery."
        }
        val probe = engine.open(buildJsonObject {
            put("join", destinationJoin); put("authorSecret", oldWallet.getValue("deviceSecret"))
            publicRoute?.standbyFor(destinationJoin)?.let { put("fullHomeStandby", it) }
            put("nextSequence", oldWallet.getValue("nextSequence"))
        }).getValue("handle").jsonPrimitive.long
        val known = try {
            engine.request(probe, buildJsonObject { put("type", "accountLookup"); put("account", account) }).getValue("chain").jsonArray
        } finally { withContext(NonCancellable) { engine.close(probe) } }
        fun compatible(left: JsonArray, right: JsonArray) = left.take(minOf(left.size, right.size)) == right.take(minOf(left.size, right.size))
        val backupChain = oldWallet.getValue("chain").jsonArray
        check(compatible(backupChain, known)) { "Destination has conflicting account history." }
        val savedChain = saved?.getValue("chain")?.jsonArray
        check(savedChain == null || compatible(savedChain, known) && compatible(savedChain, backupChain)) { "Saved account history conflicts with this recovery." }
        val latest = listOfNotNull(backupChain, known, savedChain).maxBy { it.size }
        val latestRecord = latest.last().jsonObject.getValue("payload").jsonObject
        val sameMigration = saved != null && latest.size > backupChain.size && savedChain == latest &&
            latestRecord.getValue("home").jsonPrimitive.content == destination && record(saved).getValue("home").jsonPrimitive.content == destination
        val wallet = if (sameMigration) checkNotNull(saved) else engine.portability(buildJsonObject {
            put("type", "recover"); put("rootSecret", oldWallet.getValue("rootSecret")); put("chain", latest); put("home", destination)
        }).getValue("wallet").jsonObject
        connectLocked(joinText, alias, Identity(wallet.getValue("deviceSecret").jsonPrimitive.content,
            wallet.getValue("nextSequence").jsonPrimitive.long, wallet))
        try {
            val reconciled = engine.request(checkNotNull(handle), buildJsonObject {
                put("type", "accountImport"); put("wallet", wallet); put("events", archive.getValue("events"))
            })
            val metadata = archive["localData"] as? JsonObject ?: JsonObject(emptyMap())
            val drafts = (cache.recoveryLocalData?.get("recoveredPendingEvents")?.jsonArray.orEmpty() +
                metadata["recoveredPendingEvents"]?.jsonArray.orEmpty() + recoveredDrafts)
                .filterIsInstance<JsonObject>()
                .filter { it["id"] is JsonPrimitive && (it["payload"] as? JsonObject)?.get("action") is JsonObject }
                .distinctBy { it["id"] }
            val locality = (metadata["locality"] as? JsonObject)?.let {
                runCatching { json.decodeFromJsonElement<GroupLocality>(it) }.getOrNull()
            }
            val local = JsonObject(metadata + ("recoveredPendingEvents" to JsonArray(drafts)))
            val community = cache.target!!.substringAfter('/')
            val preferences = local["preferences"]?.jsonArray.orEmpty().map { it.jsonObject }
                .filter { it["community"]?.jsonPrimitive?.content == community }
            val source = local["sourceHomeAddress"]?.jsonObject
            val notices = if (source != null && source["id"]?.jsonPrimitive?.content != destination) {
                val notice = buildJsonObject { put("type", "accountNotify"); put("address", source); put("chain", wallet.getValue("chain")) }
                (cache.accountNotices.filterNot { it["address"] == source } + notice).takeLast(64)
            } else cache.accountNotices
            val imported = reconciled.getValue("events").jsonArray.map { it.jsonObject }
            val removed = imported.filter { it["deletion"] != null }.map { it.getValue("event").jsonObject.getValue("id").jsonPrimitive.content }.toSet()
            val owned = (imported + cache.ownedEvents).distinctBy { it.getValue("event").jsonObject.getValue("id") }
            check(owned.size <= 10_000) { "Imported authored history exceeds local capacity." }
            commit(cache.copy(deleted = cache.deleted + removed, recoveryLocalData = local, ownedEvents = owned, accountNotices = notices, bookmarks = cache.bookmarks + preferences.flatMap { it["bookmarks"]?.jsonArray.orEmpty().map { value -> value.jsonPrimitive.content } }.toSet(),
                hidden = cache.hidden + preferences.flatMap { it["hidden"]?.jsonArray.orEmpty().map { value -> value.jsonPrimitive.content } }.toSet()))
            // Saved invitations from the former home cannot authorize this device at the new home.
            commitGroups(groupPreferences.copy(
                joined = groupPreferences.joined.filterValues { savedGroup ->
                    (savedGroup.join["serverAddr"] as? JsonObject)?.get("id")?.jsonPrimitive?.content == destination
                },
                home = destinationJoin,
                locality = locality ?: groupPreferences.locality
            ))
            mutableState.update { it.copy(activeGroup = null, groups = groupPreferences.joined.values.map { group -> group.summary },
                status = "Account recovered. Private authored history retained; rejoin groups to resume their feeds.") }
            local
        } catch (failure: Exception) {
            if (failure is CancellationException) throw failure
            mutableState.update { it.copy(status = "Recovery transfer paused. Retry the same encrypted archive to finish; keep the source file.") }
            throw failure
        }
    }

    suspend fun publish(body: String, topic: String, anonymous: Boolean, alias: String, headline: String = "", profile: CommunityProfile = CommunityProfile(), area: PostArea? = null) = enqueue(buildJsonObject {
        put("kind", if (headline.isBlank()) "post" else "postV2")
        if (headline.isNotBlank()) {
            area?.let { put("area", json.encodeToJsonElement(it)) }
            put("headline", headline.trim())
            if (!anonymous) put("profile", json.encodeToJsonElement(profile.cleaned()))
        }
        put("body", body.trim())
        put("topic", topic)
        put("anonymous", anonymous)
        put("displayName", if (anonymous) "Anonymous neighbor" else alias)
    })

    suspend fun reply(id: Long, body: String, anonymous: Boolean = false) {
        val parent = state.value.snapshot.posts.firstOrNull { it.id == id }?.remoteId
            ?: error("Refresh this conversation first.")
        enqueue(buildJsonObject {
            put("kind", "reply")
            put("parent", parent)
            put("body", body.trim())
            put("displayName", if (anonymous) "Anonymous neighbor" else state.value.snapshot.alias)
        })
    }

    suspend fun delete(target: String) = enqueue(buildJsonObject { put("kind", "delete"); put("target", target) })

    suspend fun vote(id: Long, direction: Int) {
        require(direction == -1 || direction == 1)
        val post = state.value.snapshot.posts.firstOrNull { it.id == id } ?: error("Post unavailable")
        val target = post.remoteId ?: error("Refresh this post first.")
        enqueue(buildJsonObject {
            put("kind", "vote")
            put("target", target)
            put("value", if (post.vote == direction) 0 else direction)
        })
    }

    suspend fun save(id: Long) = mutex.withLock {
        val post = cache.snapshot.posts.first { it.id == id }
        val remote = requireNotNull(post.remoteId)
        val bookmarks = if (remote in cache.bookmarks) cache.bookmarks - remote else cache.bookmarks + remote
        val posts = cache.snapshot.posts.map { it.copy(saved = it.remoteId in bookmarks) }
        commit(cache.copy(bookmarks = bookmarks, snapshot = cache.snapshot.copy(posts = posts)))
    }

    suspend fun hide(id: Long) = mutex.withLock {
        val remote = requireNotNull(cache.snapshot.posts.first { it.id == id }.remoteId)
        val posts = cache.snapshot.posts.map { if (it.id == id) it.copy(hidden = true) else it }
        commit(cache.copy(hidden = cache.hidden + remote, snapshot = cache.snapshot.copy(posts = posts)))
    }

    private suspend fun reportRequest(type: String, fields: JsonObjectBuilder.() -> Unit = {}): JsonObject = mutex.withLock {
        engine.request(checkNotNull(handle) { "Reconnect to your home first." }, buildJsonObject {
            put("type", type)
            fields()
        })
    }
    internal suspend fun reportDraft() = reportRequest("reportDraft")
    internal suspend fun reportStatus(receipt: String) = reportRequest("reportStatus") { put("receipt", receipt) }
    internal suspend fun submitReport(receipt: String, evidence: JsonObject) = reportRequest("submitReport") {
        put("receipt", receipt)
        put("evidence", evidence)
    }

    private suspend fun social(kind: String, active: Long = checkNotNull(handle) { "Reconnect to your home first." },
                               fields: JsonObjectBuilder.() -> Unit = {}): JsonObject =
        engine.request(active, buildJsonObject {
            put("type", "social")
            put("op", buildJsonObject { put("kind", kind); fields() })
        })

    internal suspend fun claimUsername(name: String) = mutex.withLock {
        val wallet = checkNotNull(identity?.wallet) { "Reconnect to claim your username." }
        engine.request(checkNotNull(handle), buildJsonObject { put("type", "accountRegister"); put("wallet", wallet) })
        val result = social("claimUsername") { put("name", name) }
        val registered = result.getValue("username").jsonPrimitive.content
        check(registered == "$name@${state.value.usernameDomain}") { "Your community did not confirm this username. Try again." }
        savePolicy(policy.copy(username = registered))
        if (mutableWelcomeUsername.value == WelcomeUsername.Required) mutableWelcomeUsername.value = WelcomeUsername.Confirmed
    }

    suspend fun messageContact(event: String): MessageContact = mutex.withLock {
        json.decodeFromJsonElement(social("contact") { put("event", event) })
    }

    private suspend fun refreshBlocksLocked() {
        val blocks = json.decodeFromJsonElement<List<BlockedAccount>>(social("blocks").getValue("blocks"))
        mutableState.update { it.copy(blockedAccounts = blocks) }
    }

    suspend fun refreshBlocks() = mutex.withLock { refreshBlocksLocked() }

    private suspend fun savePolicy(next: DevicePrivacy) {
        val key = checkNotNull(privacyScope) { "Connect to your home first." }
        if (next != policy) commit(cache, privacy = policies + (key to next))
        mutableState.update { it.withPrivacy(policy, cache.snapshot, history, waiting) }
    }

    suspend fun setPrivacy(update: (PrivacySettings) -> PrivacySettings) = mutex.withLock {
        savePolicy(policy.copy(settings = update(policy.settings), pendingSettings = true))
    }

    suspend fun muteThread(id: String, title: String?) = mutex.withLock {
        val muted = if (title == null) policy.mutedThreads - id else policy.mutedThreads + (id to title)
        savePolicy(policy.copy(mutedThreads = muted))
    }

    suspend fun blockAccount(value: String, byEvent: Boolean, label: String = "Blocked account", peer: String? = null) = mutex.withLock {
        val account = if (byEvent) peer else value
        if (policy.blocks.none { it.value == value || account != null && it.peer == account }) {
            val block = DeviceBlock(value, byEvent, label, account, cache.target)
            savePolicy(policy.copy(blocks = policy.blocks + block, revoked = policy.revoked + setOfNotNull(account), approved = policy.approved - setOfNotNull(account),
                pendingApprovals = policy.pendingApprovals - setOfNotNull(account)))
        }
    }

    suspend fun unblockAccount(id: String) = mutex.withLock {
        val local = policy.blocks.find { it.handle == id || "local-" + it.value == id }
        val handle = local?.handle ?: id.takeUnless { it.startsWith("local-") }
        savePolicy(policy.copy(blocks = policy.blocks - listOfNotNull(local),
            pendingUnblocks = policy.pendingUnblocks + setOfNotNull(handle?.takeUnless { it.startsWith("device-") })))
        mutableState.update { state -> state.copy(blockedAccounts = state.blockedAccounts.filterNot { it.id == id }) }
    }

    suspend fun approveContact(peer: String) = mutex.withLock {
        check(peer !in policy.blockedPeers) { "Unblock this account first." }
        savePolicy(policy.copy(revoked = policy.revoked - peer, approved = policy.approved + peer, pendingApprovals = policy.pendingApprovals + peer))
    }

    // An anonymous block must be submitted with the invitation for the original event's
    // community. A group change while offline must not strand the queued block.
    private suspend fun submitBlock(block: DeviceBlock): JsonObject {
        val current = checkNotNull(handle)
        suspend fun send(active: Long, byEvent: Boolean, value: String) = social("block", active) {
            put("target", buildJsonObject { put(if (byEvent) "event" else "account", value) })
        }
        if (!block.byEvent || block.community == cache.target) return send(current, block.byEvent, block.value)
        block.peer?.let { return send(current, false, it) }
        val origin = (listOfNotNull(groupPreferences.home) + groupPreferences.joined.values.map { it.join })
            .first { join ->
                val home = join["serverAddr"]?.jsonObject?.get("id")?.jsonPrimitive?.content
                val community = join["community"]?.jsonPrimitive?.content
                "$home/$community" == block.community && home == cache.target?.substringBefore('/')
            }
        val author = checkNotNull(identity)
        val opened = openClient(origin, author, publicRoute).getValue("handle").jsonPrimitive.long
        return try { send(opened, true, block.value) }
        finally { withContext(NonCancellable) { engine.close(opened) } }
    }

    private suspend fun syncPrivacy(advanced: Boolean): Boolean {
        var complete = true
        suspend fun attempt(action: suspend () -> Unit) {
            try { action() } catch (failure: Exception) {
                if (failure is CancellationException) throw failure
                complete = false
            }
        }
        for (id in policy.pendingUnblocks) attempt {
            social("unblock") { put("id", id) }
            savePolicy(policy.copy(pendingUnblocks = policy.pendingUnblocks - id))
        }
        for (block in policy.blocks.filter { it.handle == null }) {
            attempt {
                // Older homes return an empty acknowledgement. Match a new opaque handle only
                // when the two authenticated block lists identify it unambiguously.
                suspend fun homeBlocks() = json.decodeFromJsonElement<List<BlockedAccount>>(social("blocks").getValue("blocks"))
                val previous = if (advanced) emptySet() else homeBlocks().map { it.id }.toSet()
                val result = submitBlock(block)
                val handle = result["id"]?.jsonPrimitive?.content
                    ?: homeBlocks().singleOrNull { it.id !in previous }?.id ?: "device-" + block.value
                if (block.peer == null) {
                    val home = cache.target?.substringBefore('/')
                    fun clear(c: NetkitCache) = c.copy(snapshot = c.snapshot.copy(posts = emptyList()))
                    commit(clear(cache), archives.mapValues { (key,c) -> if (key.startsWith("$home/")) clear(c) else c })
                }
                savePolicy(policy.copy(blocks = policy.blocks.map { if (it == block) it.copy(handle = handle) else it }))
            }
        }
        if (!advanced) return complete
        if (policy.pendingSettings) attempt {
            social("setPrivacy") { put("settings", json.encodeToJsonElement(policy.settings)) }
            savePolicy(policy.copy(pendingSettings = false))
        }
        for (peer in policy.pendingApprovals) attempt {
            social("approve") { put("peer", peer) }
            savePolicy(policy.copy(pendingApprovals = policy.pendingApprovals - peer))
        }
        attempt {
            val result = social("privacyStatus")
            waiting = json.decodeFromJsonElement(result.getValue("requests"))
            val settings = if (policy.pendingSettings) policy.settings else json.decodeFromJsonElement<PrivacySettings>(result.getValue("settings"))
            val hasBlocks = result["hasBlocks"]?.jsonPrimitive?.booleanOrNull
            savePolicy(policy.copy(settings = settings, homeAllowsPublicView = hasBlocks == false,
                usernameDomain = result["usernameDomain"]?.jsonPrimitive?.contentOrNull ?: policy.usernameDomain,
                username = if ("username" in result) result["username"]?.jsonPrimitive?.contentOrNull else policy.username))
        }
        return complete
    }

    internal suspend fun privateGroupAction(action: JsonObject): JsonElement = mutex.withLock {
        val active = checkNotNull(handle) { "Connect to your home to use private groups." }
        check(state.value.enabled) { "Connect to your home to use private groups." }
        val device = checkNotNull(activeDevice) { "Open your account to use private groups." }
        val record = checkNotNull(identity?.wallet) { "Open your account to use private groups." }
            .getValue("chain").jsonArray.last().jsonObject.getValue("payload").jsonObject
        val home = record.getValue("home").jsonPrimitive.content
        val key = "private-groups-v1-$home-$device"
        val saved = secret(key)?.let { json.parseToJsonElement(it) } ?: JsonNull
        val response = engine.request(active, buildJsonObject {
            put("type", "privateGroup")
            put("state", saved)
            put("action", action)
        })
        val next = response.getValue("state")
        if (saved != next) withContext(NonCancellable) { writeSecret(key, next.toString()) }
        response.getValue("result")
    }

    private fun messageKey(): String {
        val latest = checkNotNull(identity?.wallet).getValue("chain").jsonArray.last().jsonObject.getValue("payload").jsonObject
        val owner = latest.getValue("account").jsonPrimitive.content
        val home = latest.getValue("home").jsonPrimitive.content
        val device = checkNotNull(activeDevice)
        return "direct-v1-$home-$owner-$device"
    }

    private suspend fun messageStep(action: JsonObject): Boolean {
        val next = privateMessages.step(checkNotNull(handle), messageKey(), action)
        showMessages(next)
        return next["delivery_paused"]?.jsonPrimitive?.booleanOrNull != true
    }

    private fun showMessages(next: JsonObject) {
        history = json.decodeFromJsonElement<List<DirectMessage>>(next.getValue("messages"))
        val unreadable = next.getValue("unreadable").jsonPrimitive.int
        mutableState.update { it.withPrivacy(policy, cache.snapshot, history, waiting).copy(directAvailable = next.canUseMailbox, mailboxElsewhere = next.mailboxElsewhere, messageStatus = when {
            next.mailboxElsewhere -> OTHER_MESSAGING_DEVICE
            next["delivery_paused"]?.jsonPrimitive?.booleanOrNull == true -> "Delivery paused. Yap will retry on the next sync."
            unreadable > 0 -> "Some messages could not be decrypted on this device."
            else -> null
        }) }
    }

    suspend fun sendMessage(contact: MessageContact, body: String) = mutex.withLock {
        check(!state.value.mailboxElsewhere) { OTHER_MESSAGING_DEVICE }
        check(state.value.directAvailable) { "Connect to a home that supports private messages." }
        check(contact.peer !in policy.blockedPeers) { "Unblock this account first." }
        messageStep(buildJsonObject {
            put("kind", "prepare"); put("peer", contact.peer); put("label", contact.label)
            put("name", cache.snapshot.alias); put("body", body.trim())
        })
        savePolicy(policy.copy(revoked = policy.revoked - contact.peer, approved = policy.approved + contact.peer))
    }

    suspend fun setAlias(alias: String) = mutex.withLock {
        val clean = alias.trim()
        require(clean.length in 2..30)
        commit(cache.copy(snapshot = cache.snapshot.copy(alias = clean)))
    }

    suspend fun discardPending() = mutex.withLock {
        check(loadError == null) { "Restore the unreadable network cache before changing it." }
        val posts = cache.snapshot.posts.filterNot { it.pending }
        commit(cache.copy(outbox = emptyList(), snapshot = cache.snapshot.copy(posts = posts)))
        mutableState.update { it.copy(status = "Pending uploads discarded. Sync to refresh cached votes and replies.") }
    }

    /** Upload a bounded batch and refresh the authenticated home view; errors retain exact retry IDs. */
    private var nearby = NearbyPreference()
    suspend fun setNearby(value: NearbyPreference) = mutex.withLock { nearby = value }

    suspend fun sync() = mutex.withLock {
        if (state.value.enabled && foreground.value && handle != null) syncLocked(background = false)
        Unit
    }

    /** A single OS-granted pass. Cold launches use only an existing enabled connection and
     * its persisted identity. The foreground loop and this worker always share the same lock. */
    suspend fun backgroundRefresh(preference: NearbyPreference): Boolean? = mutex.withLock {
        val saved = connectionPreference()
        if (saved?.enabled != true) return@withLock null
        val openedHere = handle == null
        try {
            if (openedHere) {
                val existing = secret(IDENTITY_KEY) ?: return@withLock false
                connectLocked(saved.join.toString(), cache.snapshot.alias, json.decodeFromString<Identity>(existing), saved.homeLabel)
            }
            nearby = preference
            syncLocked(background = true)
        } finally {
            if (openedHere || !foreground.value) withContext(NonCancellable) { closeActiveHandle() }
        }
    }

    private suspend fun syncLocked(background: Boolean): Boolean {
        fun mayContinue() = background || foreground.value
        var complete = true
        mutableState.update { it.copy(busy = true, status = "Syncing community…") }
        try {
            refreshPublicRoute()
            val active = checkNotNull(handle)
            val replica = publicRoute?.takeIf { it.target == cache.target && it.fullHomeStandby == null }?.replica
            val initialFeed = replica?.let {
                engine.request(active, buildJsonObject { put("type", "feedWithFailover"); put("replica", it) })
            }
            suspend fun learnDeletions(feed: JsonObject?) {
                val removed = feed?.get("deleted")?.jsonArray?.map { it.jsonPrimitive.content }?.toSet().orEmpty()
                if (!cache.deleted.containsAll(removed)) commit(cache.purgeDeleted(removed))
            }
            learnDeletions(initialFeed)
            if (initialFeed?.get("type")?.jsonPrimitive?.content == "publicReplicaFeed") {
                if (!mayContinue()) return false
                val status = if (policy.allowsPublicReplica) {
                    val remote = initialFeed.getValue("posts").jsonArray.map { json.decodeFromJsonElement<WirePost>(it) }
                    val verified = initialFeed.getValue("asOf").jsonPrimitive.long
                    commit(cache.withFeed(remote, verified))
                    val minutes = ((kotlin.time.Clock.System.now().epochSeconds - verified) / 60).coerceAtLeast(0)
                    val freshness = when {
                        minutes < 1 -> "updated just now"
                        minutes < 60 -> "updated ${minutes}m ago"
                        minutes < 1440 -> "updated ${minutes / 60}h ago"
                        else -> "updated ${minutes / 1440}d ago"
                    }
                    "Backup public feed · $freshness."
                } else "Showing saved posts to respect your privacy filters."
                mutableState.update { it.copy(online = false, feedUnavailable = true, directAvailable = false,
                    status = "$status Queued posts send when your home reconnects.",
                    messageStatus = if (it.mailboxElsewhere) OTHER_MESSAGING_DEVICE else "Messages will sync when your home reconnects.") }
                return false
            }
            val hasUploads = cache.outbox.isNotEmpty()
            // Stop on the first failure; keep its exact signed event for an idempotent retry.
            repeat(minOf(cache.outbox.size, if (background) 3 else MAX_UPLOADS_PER_SYNC)) {
                if (!mayContinue()) return false
                val event = cache.outbox.firstOrNull() ?: return@repeat
                val receipt = engine.request(active, buildJsonObject { put("type", "submit"); put("event", event) })
                val envelope = receipt["envelope"]?.jsonObject
                val owned = (listOfNotNull(receipt["erased"] as? JsonObject, envelope) + cache.ownedEvents)
                    .distinctBy { it.getValue("event").jsonObject.getValue("id") }
                check(owned.size <= 10_000) { "Local authored history is full. Export and archive it before publishing more." }
                // One durable commit retains the acceptance before acknowledging/removing the upload.
                val action = event.getValue("payload").jsonObject.getValue("action").jsonObject
                val removed = when {
                    action["kind"]?.jsonPrimitive?.content == "delete" -> setOf(action.getValue("target").jsonPrimitive.content)
                    envelope?.get("deletion") != null -> setOf(event.getValue("id").jsonPrimitive.content)
                    else -> emptySet()
                }
                commit(cache.copy(outbox = cache.outbox.drop(1), ownedEvents = owned).purgeDeleted(removed))
            }
            if (!mayContinue()) return false
            var feed = if (initialFeed != null && !hasUploads) initialFeed else
                engine.request(active, buildJsonObject { put("type", "feed") })
            val advanced = feed["privacy"]?.jsonPrimitive?.booleanOrNull == true
            if (feed["direct"]?.jsonPrimitive?.booleanOrNull == true && identity?.wallet != null) {
                val changed = (advanced && policy.pendingSettings) || policy.blocks.any { it.handle == null } || policy.pendingUnblocks.isNotEmpty()
                complete = syncPrivacy(advanced)
                if (changed) feed = engine.request(active, buildJsonObject { put("type", "feed") })
            }
            learnDeletions(feed)
            val chain = feed["accountChain"]?.jsonArray
            identity?.wallet?.let { wallet ->
                if (chain != null && chain.isNotEmpty() && chain != wallet["chain"]) {
                    val updated = checkNotNull(identity).copy(wallet = JsonObject(wallet + ("chain" to chain)))
                    writeSecret(IDENTITY_KEY, json.encodeToString(updated)); identity = updated
                }
            }
            val remote = feed.getValue("posts").jsonArray.map { json.decodeFromJsonElement<WirePost>(it) }
            if (!mayContinue()) return false
            val local = if (nearby.center != null && state.value.activeGroup == null) {
                val scoped = engine.request(active, buildJsonObject {
                    put("type", "nearbyFeed"); put("nearby", json.encodeToJsonElement(nearby))
                })
                learnDeletions(scoped)
                scoped.getValue("posts").jsonArray.map { json.decodeFromJsonElement<WirePost>(it) }
            } else emptyList()
            commit(cache.withFeed((remote + local).distinctBy { it.id }.sortedByDescending { it.acceptedAt }))
            val status = if (cache.outbox.isEmpty()) "Connected · ${state.value.homeLabel ?: "federated community"}"
                else "Connected · ${cache.outbox.size} pending upload(s)"
            mutableState.update { it.copy(online = true, feedUnavailable = false, status = status) }
            val available = feed["direct"]?.jsonPrimitive?.booleanOrNull == true && identity?.wallet != null
            mutableState.update { it.copy(directAvailable = available && it.directAvailable, supportsPrivacy = advanced) }
            // A recovered account can still sync posts and privacy settings on this device.
            // Recheck its mailbox once per connection without retrying a permanent rejection on each poll.
            if (available && mayContinue() && (checkMailboxRegistration || !state.value.mailboxElsewhere)) {
                try {
                    val sync = buildJsonObject { put("kind", "sync"); put("blocked", json.encodeToJsonElement(policy.blockedPeers)) }
                    val limit = if (background) 4 else 1
                    for (pass in 1..limit) {
                        val delivered = messageStep(sync)
                        checkMailboxRegistration = false
                        if (state.value.mailboxElsewhere) break
                        complete = complete && delivered
                        if (!delivered || state.value.messages.none { it.status == "queued" }) break
                    }
                } catch (failure: Exception) {
                    if (failure is CancellationException) throw failure
                    complete = false
                    mutableState.update { it.copy(messageStatus = failure.message ?: "Messages could not sync. Try again.") }
                }
            }
            if (!mayContinue()) return false
            cache.accountNotices.firstOrNull()?.let { notice ->
                try {
                    engine.request(active, notice)
                    commit(cache.copy(accountNotices = cache.accountNotices.drop(1)))
                } catch (failure: Exception) {
                    if (failure is CancellationException) throw failure
                    complete = false
                    mutableState.update { it.copy(status = "$status · old home update pending") }
                }
            }
            return complete
        } catch (error: Exception) {
            if (error is CancellationException) throw error
            mutableState.update { it.copy(online = false, feedUnavailable = true, status = "Sync paused: ${error.message ?: "connection unavailable"}. Pending uploads are kept.") }
            return false
        } finally {
            mutableState.update { it.copy(busy = false) }
        }
    }
}

/** The desktop poller belongs to the application, so hiding its last window keeps it alive. */
internal suspend fun NetkitController.pollWhileOpen(snapshot: () -> FeedSnapshot, windowFocused: StateFlow<Boolean>? = null) {
    val initial = snapshot()
    setNearby(initial.nearby)
    welcome.first { pending -> !pending }
    if (!hasOpenConnection()) restore(initial.alias)
    while (currentCoroutineContext().isActive) {
        if (state.value.enabled) sync()
        if (windowFocused == null) {
            delay(15_000)
        } else {
            val focused = windowFocused.value
            withTimeoutOrNull(if (focused) 15_000L else 120_000L) {
                windowFocused.first { it != focused }
            }
        }
        if (state.value.awaitingDefault) restore(snapshot().alias)
    }
}
