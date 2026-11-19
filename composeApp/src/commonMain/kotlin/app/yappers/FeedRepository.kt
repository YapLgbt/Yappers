package app.yappers

import kotlin.random.Random
import kotlinx.serialization.Serializable
import kotlinx.serialization.encodeToString
import kotlinx.serialization.json.Json

const val POST_LIMIT = 500
const val HEADLINE_LIMIT = 80
val topics = listOf("All", "Chatter", "Friends", "Missed connections", "Plans")

private val nameAdjectives = "Amber Apricot Breezy Bright Brisk Calm Cherry Clover Coral Cozy Dandy Dusky Ember Fuzzy Golden Hazy Honey Indigo Jolly Lilac Lunar Merry Misty Mossy Peachy Plum Quiet Rosy Silver Sleepy Soft Sunny Velvet Violet".split(' ')
private val nameNouns = "Aster Badger Bluebird Bumblebee Chaffinch Clover Comet Cricket Daffodil Dahlia Dandelion Finch Firefly Fox Gecko Heron Kestrel Lantern Linnet Lynx Maple Marigold Meteor Minnow Moth Otter Pebble Petal Puffin Robin Sparrow Sprout Starling Thistle Willow Wren".split(' ')

/** A display suggestion is local and carries no account or location information. */
internal fun suggestName(previous: String = ""): String {
    val total = nameAdjectives.size * nameNouns.size
    val parts = previous.trim().split(' ', limit = 2)
    val adjective = nameAdjectives.indexOf(parts.first())
    val noun = nameNouns.indexOf(parts.getOrNull(1))
    val old = if (adjective >= 0 && noun >= 0) adjective * nameNouns.size + noun else -1
    var index = Random.nextInt(if (old >= 0) total - 1 else total)
    if (old >= 0 && index >= old) index++
    return "${nameAdjectives[index / nameNouns.size]} ${nameNouns[index % nameNouns.size]}"
}

@Serializable
data class Reply(val id: Long, val author: String, val body: String, val remoteId: String? = null, val pending: Boolean = false, val mine: Boolean = false, val visibility: AuthorVisibility? = null, val username: String? = null, val profile: CommunityProfile? = null)
@Serializable
data class Post(
    val id: Long, val author: String, val body: String, val topic: String,
    val minutesAgo: Int = 0, val distance: String = "Nearby", val score: Int = 0,
    val vote: Int = 0, val saved: Boolean = false, val mine: Boolean = false,
    val hidden: Boolean = false, val replies: List<Reply> = emptyList(),
    val remoteId: String? = null, val pending: Boolean = false,
    val headline: String = "", val profile: CommunityProfile? = null, val area: PostArea? = null,
    val visibility: AuthorVisibility? = null, val username: String? = null, val acceptedAt: Long? = null
)
@Serializable
data class FeedSnapshot(val posts: List<Post>, val nextId: Long = 100, val alias: String = "Tender comet",
                        val profile: CommunityProfile = CommunityProfile(), val nearby: NearbyPreference = NearbyPreference(),
                        val demoBlocks: Map<String, String> = emptyMap(), val privacy: DevicePrivacy = DevicePrivacy(),
                        val replicaAsOf: Long? = null, val namedPostingScopes: Set<String> = emptySet(), val profileRevision: Long = 0)

interface LocalStorage { fun read(): String?; fun write(value: String) }

/** Fictional, device-local demo repository. Live federation is owned by NetkitController. */
class FeedRepository(private val storage: LocalStorage) {
    private val json = Json { ignoreUnknownKeys = true }
    var snapshot: FeedSnapshot = storage.read()?.let {
        runCatching { json.decodeFromString<FeedSnapshot>(it) }.getOrNull()
    }?.let { loaded ->
        val samples = demoPosts().associateBy { it.id }
        loaded.copy(posts = loaded.posts.map { post ->
            val sample = samples[post.id]
            if (!post.mine && post.headline.isEmpty() && sample?.body == post.body) post.copy(headline = sample.headline, profile = sample.profile) else post
        })
    } ?: FeedSnapshot(demoPosts(), alias = suggestName())
        private set

    private fun commit(next: FeedSnapshot) { storage.write(json.encodeToString(next)); snapshot = next }
    private fun update(id: Long, transform: (Post) -> Post) {
        require(snapshot.posts.any { it.id == id }) { "This post is no longer available." }
        commit(snapshot.copy(posts = snapshot.posts.map { if (it.id == id) transform(it) else it }))
    }
    fun publish(body: String, topic: String, anonymous: Boolean, headline: String = ""): Long {
        require(headline.trim().length <= HEADLINE_LIMIT) { "Keep your headline to $HEADLINE_LIMIT characters." }
        val clean = validate(body)
        require(topic in topics.drop(1)) { "Choose a topic." }
        val id = snapshot.nextId
        val post = Post(id, if (anonymous) "Anonymous neighbor" else snapshot.alias, clean, topic, mine = true,
            area = snapshot.nearby.center, headline = headline.trim(), profile = if (anonymous) null else snapshot.profile)
        commit(snapshot.copy(posts = listOf(post) + snapshot.posts, nextId = id + 1))
        return id
    }
    fun reply(id: Long, body: String, anonymous: Boolean = false) {
        val clean = validate(body)
        val reply = Reply(snapshot.nextId, if (anonymous) "Anonymous neighbor" else snapshot.alias, clean, mine = true, profile = if (anonymous) null else snapshot.profile)
        val post = snapshot.posts.first { it.id == id }
        require(!post.hidden) { "This conversation is hidden." }
        commit(snapshot.copy(nextId = snapshot.nextId + 1, posts = snapshot.posts.map {
            if (it.id == id) it.copy(replies = it.replies + reply) else it
        }))
    }
    fun vote(id: Long, direction: Int) {
        require(direction == 1 || direction == -1)
        update(id) { val next = if (it.vote == direction) 0 else direction; it.copy(score = it.score - it.vote + next, vote = next) }
    }
    fun save(id: Long) = update(id) { it.copy(saved = !it.saved) }
    fun hide(id: Long) = update(id) { it.copy(hidden = true) }
    fun setPrivacy(value: DevicePrivacy) = commit(snapshot.copy(privacy = value))
    fun setDemoBlock(id: String, label: String?) {
        val blocks = if (label == null) snapshot.demoBlocks - id else snapshot.demoBlocks + (id to label)
        commit(snapshot.copy(demoBlocks = blocks))
    }
    fun setAlias(value: String) {
        val clean = value.trim()
        require(clean.length in 2..30) { "Use a name between 2 and 30 characters." }
        commit(snapshot.copy(alias = clean))
    }
    /** Refresh named demo bylines when the local profile changes. */
    fun setProfile(alias: String, profile: CommunityProfile, initialNamedScope: String? = null) {
        val clean = alias.trim()
        require(clean.length in 2..30) { "Use a name between 2 and 30 characters." }
        val validated = profile.cleaned()
        val namedScopes = initialNamedScope?.let { snapshot.namedPostingScopes + it } ?: snapshot.namedPostingScopes
        val posts = snapshot.posts.map { post ->
            val replies = post.replies.map { reply ->
                if (reply.mine && reply.author != "Anonymous neighbor") reply.copy(author = clean, profile = validated) else reply
            }
            if (post.mine && post.author != "Anonymous neighbor") post.copy(author = clean, profile = validated, replies = replies)
            else post.copy(replies = replies)
        }
        commit(snapshot.copy(alias = clean, profile = validated, posts = posts, namedPostingScopes = namedScopes))
    }
    fun setPostingIdentity(scope: String, anonymous: Boolean) {
        val named = snapshot.namedPostingScopes
        commit(snapshot.copy(namedPostingScopes = if (anonymous) named - scope else named + scope))
    }
    fun setNearby(value: NearbyPreference) = commit(snapshot.copy(nearby = value))
    fun reset() = commit(FeedSnapshot(demoPosts(), alias = snapshot.alias, profile = snapshot.profile, nearby = snapshot.nearby))
    private fun validate(value: String): String {
        val clean = value.trim()
        require(clean.isNotEmpty()) { "Write a little something first." }
        require(clean.length <= POST_LIMIT) { "Keep it to $POST_LIMIT characters." }
        return clean
    }
}

// Stable fixture IDs keep identically named anonymous sample authors separate.
internal fun FeedSnapshot.withDemoBlocks(): FeedSnapshot = if (demoBlocks.isEmpty()) this else
    copy(posts = posts.filterNot { "post-${it.id}" in demoBlocks }.map { post ->
        post.copy(replies = post.replies.filterNot { "reply-${it.id}" in demoBlocks })
    })

/** Keep source chronology for New; persistent UI IDs are not timestamps in live feeds. */
fun visiblePosts(snapshot: FeedSnapshot, topic: String = "All", popular: Boolean = false, savedOnly: Boolean = false): List<Post> {
    val posts = snapshot.posts.filter { !it.hidden && (topic == "All" || it.topic == topic) && (!savedOnly || it.saved) }
    return if (popular) posts.sortedByDescending { it.score } else posts
}

fun demoPosts() = listOf(
    Post(6, "Soft landing", "A very low-stakes invitation: bring your book to the park and read next to other people who also want to make friends without making small talk.", "Friends", 4, "0.4 mi", 28,
        replies = listOf(Reply(20, "Moss enthusiast", "Parallel play but make it a book club. I'm in."), Reply(21, "Sunday person", "Can we bring a blanket and an unreasonable amount of snacks?"))),
    Post(5, "Anonymous neighbor", "To whoever keeps putting tiny crocheted hats on the fence posts: this neighborhood is better because you're in it.", "Chatter", 12, "0.8 mi", 64,
        replies = listOf(Reply(22, "Pocket sunshine", "The little blue one absolutely made my morning."))),
    Post(4, "Fern friend", "Looking for a queer climbing buddy. Very good at cheering you on. Very average at climbing.", "Friends", 23, "1.2 mi", 17),
    Post(3, "Anonymous neighbor", "You: green jacket, oat latte, reading the last page first. Me: deeply concerned, but intrigued. Same café tomorrow?", "Missed connections", 38, "0.6 mi", 39),
    Post(2, "Night picnic", "Soup night this Thursday? Thinking a potluck where everyone brings a soup and a deeply unserious opinion.", "Plans", 54, "1.6 mi", 22),
    Post(1, "Little orbit", "Being an adult is just finding new places to sit with your friends. Anyway, does anyone know a good bench?", "Chatter", 70, "2 mi", 45)
).map { post ->
    post.copy(headline = when (post.id) {
        6L -> "A book club for the quietly social"
        5L -> "To the tiny hat fairy"
        4L -> "Belay buddies, big feelings"
        3L -> "You read the last page first"
        2L -> "Soup & unserious opinions"
        else -> "In search of a really good bench"
    }, profile = if (post.author == "Anonymous neighbor") null else CommunityProfile(
        pronouns = if (post.id == 4L) "they / them" else "she / they",
        bio = "Finding little ways to make a big city feel like home.",
        identity = "Queer", interests = "Books, chosen family, long walks", hereFor = listOf("Friends", "Community")
    ))
}
