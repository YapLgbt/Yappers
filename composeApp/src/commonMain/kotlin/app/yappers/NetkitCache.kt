package app.yappers

import kotlinx.serialization.Serializable
import kotlinx.serialization.json.*

/** Persisted community state; source refactors must preserve this versioned disk schema. */
@Serializable
internal data class NetkitCache(
    val version: Int = 1,
    val target: String? = null,
    val snapshot: FeedSnapshot = FeedSnapshot(emptyList()),
    val ids: Map<String, Long> = emptyMap(),
    val nextId: Long = 1,
    val outbox: List<JsonObject> = emptyList(),
    val deleted: Set<String> = emptySet(),
    val bookmarks: Set<String> = emptySet(),
    val hidden: Set<String> = emptySet(),
    // Public signed updates awaiting delivery to former homes, never recovery keys.
    val accountNotices: List<JsonObject> = emptyList(),
    // Exact signed acceptances survive acknowledgement, home loss, and offline exports.
    val ownedEvents: List<JsonObject> = emptyList(),
    val recoveryLocalData: JsonObject? = null
)

/** Archives are keyed by home/community; only current may contain a pending outbox. */
@Serializable
internal data class NetkitDiskState(
    val version: Int = 1,
    val current: NetkitCache,
    val communities: Map<String, NetkitCache> = emptyMap(),
    val privacy: Map<String, DevicePrivacy> = emptyMap()
)

@Serializable
internal data class WireReply(val id: String, val author: String, val body: String, val mine: Boolean = false, val visibility: AuthorVisibility? = null, val username: String? = null)

/** Authenticated home projection, not an independently verified inclusion proof. */
@Serializable
internal data class WirePost(
    val id: String,
    val author: String,
    val body: String,
    val topic: String,
    val score: Int,
    val vote: Int,
    val mine: Boolean,
    val replies: List<WireReply>,
    val acceptedAt: Long,
    val headline: String = "",
    val profile: CommunityProfile? = null,
    val username: String? = null,
    val visibility: AuthorVisibility? = null,
    val area: PostArea? = null
)

/** IDs exist for Compose item identity only; they never encode chronological order. */
private class LocalIds(cache: NetkitCache) {
    val ids = cache.ids.toMutableMap()
    var nextId = cache.nextId
        private set

    fun allocate(remote: String): Long = ids.getOrPut(remote) { nextId++ }
}

/** Apply home-confirmed deletions before serving cached or restored content. */
internal fun NetkitCache.purgeDeleted(learned: Set<String>): NetkitCache {
    val known = deleted + learned
    if (known.isEmpty()) return this
    val posts = snapshot.posts.filterNot { it.remoteId in known }.map { post ->
        post.copy(replies = post.replies.filterNot { it.remoteId in known })
    }
    val drafts = recoveryLocalData?.get("recoveredPendingEvents") as? JsonArray
    val local = if (drafts == null) recoveryLocalData else JsonObject(recoveryLocalData + ("recoveredPendingEvents" to
        JsonArray(drafts.filterNot { it.jsonObject["id"]?.jsonPrimitive?.content in known })))
    return copy(deleted = known, snapshot = snapshot.copy(posts = posts), outbox = outbox.filterNot { it["id"]?.jsonPrimitive?.content in known },
        bookmarks = bookmarks - known, recoveryLocalData = local, ownedEvents = ownedEvents.filterNot {
            it["event"]?.jsonObject?.get("id")?.jsonPrimitive?.content in known && it["deletion"] == null
        })
}

/** Apply one immutable queued event to the local view without publishing it. */
internal fun NetkitCache.overlay(event: JsonObject): NetkitCache {
    val remoteId = event.getValue("id").jsonPrimitive.content
    val action = event.getValue("payload").jsonObject.getValue("action").jsonObject
    val allocator = LocalIds(this)
    val posts = snapshot.posts.toMutableList()
    when (action.getValue("kind").jsonPrimitive.content) {
        "post", "postV2" -> if (posts.none { it.remoteId == remoteId }) {
            posts.add(0, Post(
                id = allocator.allocate(remoteId),
                author = action.getValue("displayName").jsonPrimitive.content,
                body = action.getValue("body").jsonPrimitive.content,
                topic = action.getValue("topic").jsonPrimitive.content,
                headline = action["headline"]?.jsonPrimitive?.content ?: "",
                area = action["area"]?.let { Json.decodeFromJsonElement<PostArea>(it) },
                profile = action["profile"]?.let { Json.decodeFromJsonElement<CommunityProfile>(it) },
                distance = "Pending upload",
                mine = true,
                remoteId = remoteId,
                pending = true,
                saved = remoteId in bookmarks,
                hidden = remoteId in hidden
            ))
        }
        "reply" -> {
            val target = action.getValue("parent").jsonPrimitive.content
            val index = posts.indexOfFirst { it.remoteId == target }
            if (index >= 0 && posts[index].replies.none { it.remoteId == remoteId }) {
                val reply = Reply(
                    id = allocator.allocate(remoteId),
                    author = action.getValue("displayName").jsonPrimitive.content,
                    body = action.getValue("body").jsonPrimitive.content,
                    remoteId = remoteId,
                    pending = true
                )
                posts[index] = posts[index].copy(replies = posts[index].replies + reply)
            }
        }
        "vote" -> {
            val target = action.getValue("target").jsonPrimitive.content
            val value = action.getValue("value").jsonPrimitive.int
            val index = posts.indexOfFirst { it.remoteId == target }
            if (index >= 0) {
                val post = posts[index]
                posts[index] = post.copy(score = post.score - post.vote + value, vote = value)
            }
        }
    }
    return copy(
        ids = allocator.ids,
        nextId = allocator.nextId,
        snapshot = snapshot.copy(posts = posts)
    )
}

/** Replace the home projection, retain local preferences, then overlay unacknowledged events. */
internal fun NetkitCache.withFeed(remote: List<WirePost>, replicaAsOf: Long? = null): NetkitCache {
    val allocator = LocalIds(this)
    val previous = if (replicaAsOf != null) snapshot.posts.associateBy { it.remoteId } else emptyMap()
    val posts = remote.map { post ->
        val prior = previous[post.id]
        val ownReplies = prior?.replies.orEmpty().filter { it.mine }.map { it.remoteId }.toSet()
        Post(
            id = allocator.allocate(post.id),
            remoteId = post.id,
            author = post.author,
            body = post.body,
            topic = post.topic,
            headline = post.headline, profile = post.profile, area = post.area, acceptedAt = post.acceptedAt,
            distance = "Community",
            score = post.score,
            vote = prior?.vote ?: post.vote,
            mine = post.mine || prior?.mine == true, visibility = post.visibility, username = post.username,
            saved = post.id in bookmarks,
            hidden = post.id in hidden,
            replies = post.replies.map { reply ->
                Reply(allocator.allocate(reply.id), reply.author, reply.body, remoteId = reply.id, mine = reply.mine || reply.id in ownReplies, visibility = reply.visibility, username = reply.username)
            }
        )
    }
    val refreshed = copy(
        ids = allocator.ids,
        nextId = allocator.nextId,
        snapshot = snapshot.copy(posts = posts, replicaAsOf = replicaAsOf)
    )
    return outbox.fold(refreshed) { current, event -> current.overlay(event) }
}
