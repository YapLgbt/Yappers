package app.yappers

import kotlinx.serialization.json.JsonObject

/**
 * Platform persistence: protected atomic files for caches, Keychain for credentials.
 *
 * Reads return literal `"null"` for absence. Nonoptional throwing strings distinguish
 * missing data from Objective-C NSError failures when Swift implements this interface.
 * Writes must return only after durable persistence; callers acknowledge UI actions afterward.
 */
interface NetkitStorage {
    @Throws(Exception::class)
    fun readState(): String
    @Throws(Exception::class)
    fun writeState(value: String)
    @Throws(Exception::class)
    fun readSecret(key: String): String
    @Throws(Exception::class)
    fun writeSecret(key: String, value: String)
}

/**
 * Application-specific Rust ABI, expressed as JSON to keep Rust layouts out of Kotlin.
 * Implementations must dispatch blocking native work off the UI thread. The caller owns
 * each returned handle until [close]; preparing an event never sends it to the network.
 */
interface NetkitEngine {
    val supportsPortability: Boolean get() = false
    suspend fun identity(): JsonObject
    /** Offline signing/recovery; results can contain recovery keys. Never log or send them as RPCs. */
    suspend fun portability(command: JsonObject): JsonObject = error("Portable accounts are unavailable in this engine.")
    suspend fun open(config: JsonObject): JsonObject
    suspend fun request(handle: Long, command: JsonObject): JsonObject
    suspend fun close(handle: Long)
}

/** Immutable observable state; live data and the local demo have separate stores. */
data class NetkitState(
    val enabled: Boolean = false,
    val awaitingDefault: Boolean = false,
    val homeLabel: String? = null,
    val defaultUsernameDomain: String? = null,
    val online: Boolean = false,
    val feedUnavailable: Boolean = false,
    val busy: Boolean = false,
    val status: String = "Local demo",
    val snapshot: FeedSnapshot = FeedSnapshot(emptyList()),
    val pendingCount: Int = 0,
    val recoveredDrafts: List<JsonObject> = emptyList(),
    val ownershipReady: Boolean = false,
    val ownedHistory: List<JsonObject> = emptyList(),
    val groups: List<GroupSummary> = emptyList(),
    val joinedGroups: List<GroupSummary> = emptyList(),
    val locality: GroupLocality? = null,
    val activeGroup: GroupSummary? = null,
    val hasHomeFeed: Boolean = false,
    val messagingScope: String? = null,
    val privacy: DevicePrivacy = DevicePrivacy(),
    val supportsPrivacy: Boolean = false,
    val requests: List<String> = emptyList(),
    val directAvailable: Boolean = false,
    val mailboxElsewhere: Boolean = false,
    val messages: List<DirectMessage> = emptyList(),
    val blockedAccounts: List<BlockedAccount> = emptyList(),
    val messageStatus: String? = null,
    val groupsBusy: Boolean = false,
    val groupsStatus: String = "Connect a home to discover groups."
)

internal val NetkitState.usernameDomain get() = privacy.usernameDomain ?: defaultUsernameDomain
internal val NetkitState.needsUsername get() = enabled && usernameDomain != null && privacy.username == null

internal enum class WelcomeUsername { Unchecked, Required, Confirmed, Unsupported }
