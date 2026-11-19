package app.yappers

import kotlinx.serialization.Serializable

@Serializable
data class PrivacySettings(
    val hideAnonymous: Boolean = false, val feedDays: Int = 0,
    val dmDays: Int = 0, val approvedOnly: Boolean = false
)

@Serializable
data class AuthorVisibility(val peer: String? = null, val anonymous: Boolean, val age: Int)

@Serializable
data class DeviceBlock(val value: String, val byEvent: Boolean, val label: String, val peer: String?,
                       val community: String?, val handle: String? = null)

@Serializable
data class DevicePrivacy(
    val settings: PrivacySettings = PrivacySettings(),
    val username: String? = null,
    val usernameDomain: String? = null,
    val pendingSettings: Boolean = false,
    val homeAllowsPublicView: Boolean = false,
    val blocks: List<DeviceBlock> = emptyList(), val pendingUnblocks: Set<String> = emptySet(),
    val revoked: Set<String> = emptySet(),
    val approved: Set<String> = emptySet(), val pendingApprovals: Set<String> = emptySet(),
    val notifications: NotificationInbox = NotificationInbox(),
    val pendingReads: Map<String, Long> = emptyMap(),
    val pendingMutes: Map<String, Boolean> = emptyMap(),
    val pushCategories: Set<String> = emptySet(),
    val registeredPushToken: String? = null,
    val pushAvailable: Boolean = false,
    val mutedThreads: Map<String, String> = emptyMap()
) {
    val allowsPublicReplica get() = homeAllowsPublicView && blocks.isEmpty() && pendingUnblocks.isEmpty()
    val blockedPeers get() = blocks.mapNotNull { it.peer }.toSet()
    val hasPending get() = pendingSettings || blocks.any { it.handle == null } || pendingUnblocks.isNotEmpty() || pendingApprovals.isNotEmpty()

    fun filter(feed: FeedSnapshot): FeedSnapshot {
        val peers = blockedPeers
        val events = blocks.filter { it.byEvent }.map { it.value }.toSet()
        fun visible(id: String?, name: String, mine: Boolean, meta: AuthorVisibility?): Boolean {
            if (mine) return true
            if (feed.replicaAsOf != null && !allowsPublicReplica) return false
            return id !in events && meta?.peer !in peers &&
                !(settings.hideAnonymous && (meta?.anonymous ?: (name == "Anonymous neighbor"))) &&
                (meta?.age ?: 0) >= settings.feedDays
        }
        return feed.copy(posts = feed.posts.filter { post ->
            (post.remoteId ?: "demo-${post.id}") !in mutedThreads &&
                visible(post.remoteId, post.author, post.mine, post.visibility)
        }.map { p -> p.copy(replies = p.replies.filter { r ->
            visible(r.remoteId, r.author, r.mine || r.pending, r.visibility)
        }) })
    }
}

internal fun NetkitState.withPrivacy(
    policy: DevicePrivacy, feed: FeedSnapshot, history: List<DirectMessage>, waiting: List<String>
): NetkitState {
    val blocked = policy.blockedPeers
    val known = policy.approved + history.filter { it.outgoing && it.peer !in policy.revoked }.map { it.peer }
    val allowed = history.filter { it.peer !in blocked }
    val requests = (waiting + allowed.filter { !it.outgoing && it.peer !in known &&
        (policy.settings.approvedOnly || it.age < policy.settings.dmDays) }.map { it.peer })
        .distinct().filter { it !in blocked && it !in policy.approved }
    val restricted = requests.toSet()
    return copy(privacy = policy, snapshot = policy.filter(feed), requests = requests,
        messages = allowed.filter { it.peer !in restricted })
}
