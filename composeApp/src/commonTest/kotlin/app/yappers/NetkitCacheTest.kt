package app.yappers

import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertTrue
import kotlin.test.assertFalse
import kotlinx.serialization.json.*

class NetkitCacheTest {
    private fun post(id: String, acceptedAt: Long) = WirePost(
        id, "Neighbor", id, "Chatter", 0, 0, false, emptyList(), acceptedAt
    )

    @Test fun newFeedPreservesHomeChronologyAcrossRefreshesAndNewUiIds() {
        val first = NetkitCache().withFeed(listOf(post("newer", 20), post("older", 10)))
        assertEquals(listOf("newer", "older"), visiblePosts(first.snapshot).map { it.remoteId })

        val refreshed = first.withFeed(listOf(post("newest", 30), post("newer", 20), post("older", 10)))
        assertEquals(listOf("newest", "newer", "older"), visiblePosts(refreshed.snapshot).map { it.remoteId })
        assertEquals(first.ids.getValue("newer"), refreshed.ids.getValue("newer"))
        assertEquals(first.ids.getValue("older"), refreshed.ids.getValue("older"))
    }
    @Test fun richProjectionSurvivesRefresh() {
        val profile = CommunityProfile(pronouns = "she / they", bio = "Hello world")
        val cache = NetkitCache().withFeed(listOf(post("rich", 20).copy(headline = "Hello neighbors", profile = profile)))
        assertEquals("Hello neighbors", cache.snapshot.posts.first().headline)
        assertEquals(profile, cache.snapshot.posts.first().profile)
    }
    @Test fun queuedCommentShowsPendingUntilItAppearsInTheHomeProjection() {
        val event = Json.parseToJsonElement("""{"id":"comment","payload":{"action":{"kind":"reply","parent":"parent","displayName":"You","body":"Hello"}}}""").jsonObject
        val pending = NetkitCache().withFeed(listOf(post("parent", 1))).copy(outbox = listOf(event)).overlay(event)
        val comment = pending.snapshot.posts.first().replies.single()
        assertTrue(comment.pending)
        val acknowledged = pending.copy(outbox = emptyList()).withFeed(listOf(post("parent", 1).copy(replies = listOf(WireReply("comment", "You", "Hello")))))
        assertFalse(acknowledged.snapshot.posts.first().replies.single().pending)
        assertEquals(comment.id, acknowledged.snapshot.posts.first().replies.single().id)
    }
    @Test fun devicePrivacyFiltersCachedFeedAndInboxIndependently() {
        val named = Post(1,"Neighbor","Named","Chatter",remoteId = "one",
            visibility = AuthorVisibility("established",false,7),
            replies = listOf(Reply(11,"Anonymous neighbor","Hidden"), Reply(12,"Me","Mine",mine = true)))
        val fresh = named.copy(id = 2, remoteId = "two", visibility = AuthorVisibility("new",false,0))
        val blocked = named.copy(id = 3, remoteId = "three", visibility = AuthorVisibility("blocked",false,30))
        val anonymous = named.copy(id = 4, remoteId = "four", visibility = AuthorVisibility(null,true,30))
        val muted = named.copy(id = 5, remoteId = "five")
        val policy = DevicePrivacy(settings = PrivacySettings(hideAnonymous = true,feedDays = 7,approvedOnly = true),
            blocks = listOf(DeviceBlock("blocked",false,"Neighbor","blocked",null)), mutedThreads = mapOf("five" to "Muted"))
        val feed = FeedSnapshot(listOf(named,fresh,blocked,anonymous,muted))
        val message = DirectMessage("1","unknown","Unknown","Secret",false,"received")
        val history = listOf(message,message.copy(id = "2",peer = "blocked"),message.copy(id = "3",peer = "friend",outgoing = true))
        val projected = NetkitState().withPrivacy(policy,feed,history,listOf("blocked"))
        assertEquals(listOf("one"),projected.snapshot.posts.map { it.remoteId })
        assertEquals(listOf("Mine"),projected.snapshot.posts.single().replies.map { it.body })
        assertEquals(listOf("unknown"),projected.requests)
        assertEquals(listOf("friend"),projected.messages.map { it.peer })
        val accepted = NetkitState().withPrivacy(policy.copy(approved = setOf("unknown")),feed,history,listOf("unknown"))
        assertTrue(accepted.requests.isEmpty())
        assertEquals(listOf("unknown","friend"),accepted.messages.map { it.peer })
        val revoked = policy.copy(revoked = setOf("friend"))
        assertTrue("friend" in NetkitState().withPrivacy(revoked,feed,history + message.copy(peer = "friend"),emptyList()).requests)
    }
}
