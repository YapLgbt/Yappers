package app.yappers

import kotlin.test.*

class FeedRepositoryTest {
    private class MemoryStorage : LocalStorage {
        var value: String? = null
        override fun read() = value
        override fun write(value: String) { this.value = value }
    }
    @Test fun inboxPhotosFollowNamedPeersAndRespectPhotoRemoval() {
        val river = Post(1, "Neighbor", "Hi", "Friends", visibility = AuthorVisibility("river", false, 0),
            profile = CommunityProfile(photo = "old"), acceptedAt = 10)
        val fern = river.copy(id = 2, visibility = AuthorVisibility("fern", false, 0), profile = CommunityProfile(photo = "fern"))
        val removed = river.copy(id = 3, profile = CommunityProfile(), acceptedAt = 20)
        val anonymous = river.copy(visibility = AuthorVisibility("fern", true, 0), acceptedAt = 30)
        val hidden = river.copy(hidden = true, acceptedAt = 40)
        assertEquals(mapOf("river" to "", "fern" to "fern"), inboxPhotos(listOf(removed, fern, hidden, river, anonymous)))
    }
    @Test fun postingIdentityDefaultsToAnonymousAndIsRememberedPerAccount() {
        val storage = MemoryStorage().apply { value = """{"posts":[]}""" }
        val repo = FeedRepository(storage)
        assertTrue(repo.snapshot.namedPostingScopes.isEmpty())
        repo.setPostingIdentity("home/account", anonymous = false)
        assertEquals(setOf("home/account"), FeedRepository(storage).snapshot.namedPostingScopes)
        repo.setPostingIdentity("home/account", anonymous = true)
        assertTrue(FeedRepository(storage).snapshot.namedPostingScopes.isEmpty())
    }
    @Test fun publishingTrimsAndPersists() {
        val storage = MemoryStorage()
        val repo = FeedRepository(storage)
        val id = repo.publish("  Hello neighbors  ", "Friends", false)
        assertEquals("Hello neighbors", repo.snapshot.posts.first().body)
        assertEquals(id, FeedRepository(storage).snapshot.posts.first().id)
        assertTrue(repo.snapshot.posts.first().mine)
    }
    @Test fun nearbyPreferenceSurvivesRelaunchAndChecksDistanceAcrossCityBorders() {
        val storage = MemoryStorage()
        val repo = FeedRepository(storage)
        val center = PostArea("Boston", 4236, -7106)
        val preference = NearbyPreference(center, 10)
        repo.setNearby(preference)
        repo.publish("Hello from Boston", "Friends", true, "Hello")
        assertEquals(preference, FeedRepository(storage).snapshot.nearby)
        assertEquals(center, repo.snapshot.posts.first().area)
        assertTrue(preference.includes(PostArea("Cambridge", 4237, -7111)))
        assertFalse(preference.includes(PostArea("New York", 4071, -7401)))
        assertFalse(preference.includes(null))
        assertTrue(NearbyPreference().includes(null))
    }
    @Test fun rejectsEmptyAndOverlongContent() {
        val repo = FeedRepository(MemoryStorage())
        assertFailsWith<IllegalArgumentException> { repo.publish("  ", "Friends", true) }
        assertFailsWith<IllegalArgumentException> { repo.publish("x".repeat(501), "Friends", true) }
        assertFailsWith<IllegalArgumentException> { repo.publish("Hi", "Unknown", true) }
        assertFailsWith<IllegalArgumentException> { repo.reply(6, "\n") }
    }
    @Test fun votesToggleAndSwitchWithoutInflation() {
        val repo = FeedRepository(MemoryStorage())
        val initial = repo.snapshot.posts.first().score
        repo.vote(6, 1)
        assertEquals(initial + 1, repo.snapshot.posts.first().score)
        repo.vote(6, 1)
        assertEquals(initial, repo.snapshot.posts.first().score)
        repo.vote(6, 1); repo.vote(6, -1)
        assertEquals(initial - 1, repo.snapshot.posts.first().score)
    }
    @Test fun repliesAndHiddenPostsSurviveRestart() {
        val storage = MemoryStorage()
        val repo = FeedRepository(storage)
        repo.reply(6, "Count me in!")
        repo.save(6)
        assertEquals(1, visiblePosts(repo.snapshot, savedOnly = true).size)
        repo.hide(6)
        val restored = FeedRepository(storage)
        assertEquals("Count me in!", restored.snapshot.posts.first().replies.last().body)
        assertFalse(visiblePosts(restored.snapshot).any { it.id == 6L })
        assertTrue(visiblePosts(restored.snapshot, savedOnly = true).isEmpty())
    }
    @Test fun blockingAnAnonymousDemoAuthorPersistsWithoutHidingOtherAnonymousPosts() {
        val storage = MemoryStorage()
        val repo = FeedRepository(storage)
        repo.save(5)
        repo.setDemoBlock("post-5", "Anonymous neighbor")
        val reopened = FeedRepository(storage)
        val feed = reopened.snapshot.withDemoBlocks()
        assertFalse(visiblePosts(feed, savedOnly = true).any { it.id == 5L })
        assertTrue(visiblePosts(feed).any { it.id == 3L })
        reopened.setDemoBlock("post-5", null)
        assertTrue(visiblePosts(reopened.snapshot.withDemoBlocks(), savedOnly = true).any { it.id == 5L })
    }
    @Test fun filtersAndPopularSortUseActualData() {
        val repo = FeedRepository(MemoryStorage())
        assertTrue(visiblePosts(repo.snapshot, "Friends").all { it.topic == "Friends" })
        assertEquals(5L, visiblePosts(repo.snapshot, popular = true).first().id)
    }
    @Test fun aliasValidationAndCorruptStorageRecovery() {
        val storage = MemoryStorage().apply { value = "broken data" }
        val repo = FeedRepository(storage)
        assertEquals(6, repo.snapshot.posts.size)
        assertFailsWith<IllegalArgumentException> { repo.setAlias(" ") }
        repo.setAlias("  New neighbor  ")
        assertEquals("New neighbor", FeedRepository(storage).snapshot.alias)
    }
    @Test fun profilesPersistAndAnonymousPostsOmitEveryDetail() {
        val storage = MemoryStorage()
        val repo = FeedRepository(storage)
        val profile = CommunityProfile(pronouns = " they / them ", bio = "A quiet extrovert", identity = "Queer", hereFor = listOf("Friends"))
        repo.setProfile("  Comet  ", profile)
        repo.publish("Body", "Friends", false, " Headline ")
        val named = repo.snapshot.posts.first()
        assertEquals("Headline", named.headline)
        assertEquals("they / them", named.profile?.pronouns)
        repo.setProfile("Comet", profile.copy(bio = "New bio"))
        assertEquals("New bio", repo.snapshot.posts.first().profile?.bio)
        repo.publish("Private", "Chatter", true, "Just a thought")
        assertNull(repo.snapshot.posts.first().profile)
        val restored = FeedRepository(storage).snapshot
        assertEquals("New bio", restored.profile.bio)
        assertEquals("Comet", restored.alias)
        assertEquals(named.copy(profile = restored.profile), restored.posts[1])
        repo.reset()
        assertEquals(restored.profile, repo.snapshot.profile)
        assertEquals("Comet", repo.snapshot.alias)
        assertTrue(repo.snapshot.posts.none { it.mine })
        assertFailsWith<IllegalArgumentException> { repo.publish("Body", "Chatter", true, "x".repeat(81)) }
        assertFailsWith<IllegalArgumentException> { repo.setProfile("Comet", profile.copy(photo = "x".repeat(PROFILE_PHOTO_LENGTH + 1))) }
    }
}
