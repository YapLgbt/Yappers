package app.yappers

import kotlinx.coroutines.CompletableDeferred
import kotlinx.coroutines.launch
import kotlinx.coroutines.runBlocking
import kotlinx.serialization.json.*
import kotlin.test.*

class NetkitControllerTest {
    private class Storage : NetkitStorage {
        var data: String? = null
        val secrets = mutableMapOf<String, String>()
        var failNextStateWrite = false
        var failNextSecretWrite = false
        override fun readState() = data ?: "null"
        override fun writeState(value: String) {
            if (failNextStateWrite) { failNextStateWrite = false; error("Disk unavailable") }
            data = value
        }
        override fun readSecret(key: String) = secrets[key] ?: "null"
        override fun writeSecret(key: String, value: String) {
            if (failNextSecretWrite) { failNextSecretWrite = false; error("Keychain unavailable") }
            secrets[key] = value
        }
    }
    @Test fun submittedSignupProfileSurvivesInterruptedStorageWrites(): Unit = runBlocking {
        val storage = Storage()
        val id = "a".repeat(64)
        val manifest = """{"version":1,"join":{"serverAddr":{"id":"$id"},"community":"$id/public","networkMode":"n0"}}"""
        val controller = NetkitController(storage, Engine(Server())) { manifest }
        val profile = CommunityProfile(pronouns = "they", bio = "Bakes bread", hereFor = listOf("Friends"))
        controller.prepareWelcome(false)
        controller.finishWelcome("River", profile = profile)
        val reopened = NetkitController(storage, Engine(Server())) { manifest }
        reopened.prepareWelcome(false)
        assertTrue(reopened.backupStep.value)
        assertFailsWith<IllegalStateException> {
            reopened.persistWelcomeProfile { _, _ -> error("Profile write failed") }
        }
        storage.failNextSecretWrite = true
        assertFailsWith<IllegalStateException> {
            reopened.persistWelcomeProfile { _, restored -> assertEquals(profile, restored) }
        }
        reopened.persistWelcomeProfile { name, restored ->
            assertEquals("River", name)
            assertEquals(profile, restored)
        }
        reopened.persistWelcomeProfile { _, _ -> fail("A later edit must not be overwritten") }
    }

    @Test fun enrollmentRetriesAndRespectsExistingHomeAndDemoOptOut(): Unit = runBlocking {
        val storage = Storage()
        val id = "a".repeat(64)
        val document = """{"version":1,"join":{"serverAddr":{"id":"$id"},"community":"$id/public","networkMode":"n0"}}"""
        var calls = 0
        val controller = NetkitController(storage, Engine(Server())) {
            if (++calls == 1) error("offline")
            document
        }
        controller.restore("Neighbor")
        assertTrue(controller.state.value.awaitingDefault)
        controller.restore("Neighbor")
        assertTrue(controller.state.value.enabled)
        assertEquals("Neighbor", controller.state.value.snapshot.alias)
        controller.shutdown()
        controller.restore()
        assertEquals(2, calls)
        controller.useDemo()
        controller.restore()
        assertFalse(controller.state.value.enabled)
        assertFalse(controller.state.value.awaitingDefault)
        assertEquals(2, calls)
        controller.joinDefaultHome("Neighbor")
        assertEquals("yap.lgbt", controller.state.value.homeLabel)
        assertTrue(controller.state.value.enabled)
        assertEquals(3, calls)
        assertFails { publicHomeJoin(document.replace("/public", "/private")) }
    }

    @Test fun mailboxRegistrationDoesNotReplaceDeviceKeysOrPendingMessages() = runBlocking {
        val local = Storage()
        val key = "device-private-test"
        val saved = buildJsonObject {
            put("account", "saved test ratchet")
            put("messages", buildJsonArray { add("existing history") })
            put("outbox", buildJsonArray { add("original encrypted packet") })
        }
        local.writeSecret(key, saved.toString())
        var failure: String? = "rejected: messaging is registered to a different device"
        val engine = object : NetkitEngine by Engine(Server()) {
            override suspend fun request(handle: Long, command: JsonObject): JsonObject {
                assertEquals(saved, command.getValue("state"))
                failure?.let { error(it) }
                return buildJsonObject { put("state", saved) }
            }
        }
        val messages = PrivateMessages(local, engine)
        val sync = buildJsonObject { put("kind", "sync") }
        val rejected = messages.step(1, key, sync)
        assertTrue(rejected.mailboxElsewhere)
        assertFalse(rejected.canUseMailbox)
        for ((field, value) in saved) assertEquals(value, rejected[field])
        failure = "home unavailable"
        assertFailsWith<IllegalStateException> { messages.step(1, key, sync) }
        assertEquals(rejected, messages.read(key))
        failure = null
        assertTrue(messages.step(1, key, sync).canUseMailbox)
        assertFalse(messages.read(key)!!.mailboxElsewhere)
    }

    private class Server {
        val accepted = linkedMapOf<String, JsonObject>()
        val attempts = mutableListOf<String>()
        var offline = false
        var loseReceiptOnce = false
        val replicaCommands = mutableListOf<JsonObject>()
    }
    private class Engine(val server: Server) : NetkitEngine {
        var sequence = 1L
        var nextHandle = 1L
        val closed = mutableListOf<Long>()
        val opened = mutableListOf<JsonObject>()
        var beforeClose: (suspend () -> Unit)? = null
        override suspend fun identity() = buildJsonObject { put("secret", "private-test-identity") }
        override suspend fun open(config: JsonObject): JsonObject {
            opened += config
            sequence = config.getValue("nextSequence").jsonPrimitive.long
            return buildJsonObject {
                put("handle", nextHandle++); put("home", config.getValue("join").jsonObject["home"] ?: config.getValue("join").jsonObject["serverAddr"]?.jsonObject?.get("id") ?: JsonPrimitive("home-a")); put("community", config.getValue("join").jsonObject["community"] ?: JsonPrimitive("community"))
            }
        }
        override suspend fun close(handle: Long) {
            beforeClose?.invoke()
            closed += handle
        }
        private fun summary(id: String, details: JsonObject? = null): JsonObject = buildJsonObject {
            put("id", id); put("owner", "home-a"); put("name", details?.get("name") ?: JsonPrimitive(if (id == "local-group") "City readers" else "Readers everywhere"))
            put("description", details?.get("description") ?: JsonPrimitive("A public reading group"))
            put("category", details?.get("category") ?: JsonPrimitive("Friends")); put("homes", 2); put("hosted", true)
            put("locality", details?.get("locality") ?: if (id == "local-group") buildJsonObject { put("city", "New York"); put("region", "NY"); put("country", "US") } else JsonNull)
        }
        private fun joined(id: String, details: JsonObject? = null) = buildJsonObject {
            put("type", "groupJoined"); put("group", summary(id, details))
            put("join", buildJsonObject { put("home", "home-a"); put("community", id); put("accessToken", "private-group-capability") })
        }
        override suspend fun request(handle: Long, command: JsonObject): JsonObject {
            return when (command.getValue("type").jsonPrimitive.content) {
                "groups" -> buildJsonObject { put("type", "groups"); put("groups", buildJsonArray { add(summary("local-group")); add(summary("world-group")) }) }
                "joinGroup" -> joined(command.getValue("group").jsonPrimitive.content)
                "createGroup" -> joined("created-group", command.getValue("details").jsonObject)
                "prepare" -> buildJsonObject {
                    val id = "event-${sequence}"
                    put("event", buildJsonObject {
                        put("id", id); put("payload", buildJsonObject { put("action", command.getValue("action")); put("sequence", sequence) })
                    }); put("nextSequence", ++sequence)
                }
                "submit" -> {
                    if (server.offline) error("Network offline")
                    val event = command.getValue("event").jsonObject
                    val id = event.getValue("id").jsonPrimitive.content
                    server.attempts += id; server.accepted.putIfAbsent(id, event)
                    if (server.loseReceiptOnce) { server.loseReceiptOnce = false; error("Receipt lost") }
                    buildJsonObject { put("type", "accepted"); put("eventId", id) }
                }
                "feedWithFailover" -> {
                    server.replicaCommands += command
                    if (server.offline) Json.parseToJsonElement("""{"type":"publicReplicaFeed","eventCount":1,"asOf":1,"posts":[{"id":"backup-post","author":"A neighbor","body":"From backup","topic":"Chatter","vote":0,"score":0,"mine":false,"replies":[],"acceptedAt":1}]}""").jsonObject
                    else request(handle,buildJsonObject { put("type","feed") })
                }
                "feed" -> {
                    if (server.offline) error("Network offline")
                    buildJsonObject {
                        put("type", "feed"); put("eventCount", server.accepted.size)
                        put("posts", buildJsonArray {
                            server.accepted.values.forEach { event ->
                                val action = event.getValue("payload").jsonObject.getValue("action").jsonObject
                                if (action.getValue("kind").jsonPrimitive.content in listOf("post", "postV2")) add(buildJsonObject {
                                    put("id", event.getValue("id")); put("author", action.getValue("displayName")); put("body", action.getValue("body")); put("topic", action.getValue("topic"))
                                    put("headline", action["headline"] ?: JsonPrimitive("")); put("profile", action["profile"] ?: JsonNull)
                                    put("score", 0); put("vote", 0); put("mine", true); put("replies", buildJsonArray {}); put("acceptedAt", 1)
                                })
                            }
                        })
                    }
                }
                else -> error("Unknown command")
            }
        }
    }
    private val join = """{"home":"home-a"}"""
    @Test fun privateStandbyFollowsOnlyItsPinnedHomeAndSurvivesOfflineDiscovery() = runBlocking {
        val home = "a".repeat(64)
        val backup = "b".repeat(64)
        val primary = """{"serverAddr":{"id":"$home"},"community":"$home/public","networkMode":"n0"}"""
        val grant = """{"home":"$home","standby":{"id":"$backup","addrs":[]},"signature":"test-signature"}"""
        val manifest = """{"version":1,"join":$primary,"fullHomeStandby":$grant}"""
        val storage = Storage()
        val engine = Engine(Server())
        val first = NetkitController(storage, engine) { manifest }
        first.restore()
        val signed = Json.parseToJsonElement(grant)
        assertEquals(signed, engine.opened.last()["fullHomeStandby"])
        assertEquals(Json.parseToJsonElement(primary), engine.opened.last()["join"])
        first.shutdown()
        val reopened = NetkitController(storage, engine) { error("Offline") }
        reopened.restore()
        assertEquals(signed, engine.opened.last()["fullHomeStandby"])
        reopened.connect(primary.replace(home, "c".repeat(64)), "Neighbor")
        assertNull(engine.opened.last()["fullHomeStandby"])
        assertFails { publicEnrollment(manifest.replace(grant, grant.replace(home, backup))) }
        reopened.shutdown()
    }
    @Test fun enrolledClientDiscoversBackupAndKeepsItsOutboxUntilFailback() = runBlocking {
        val id = "a".repeat(64)
        val replica = "b".repeat(64)
        val primary = """{"serverAddr":{"id":"$id"},"community":"$id/public","accessToken":"primary-only","networkMode":"n0"}"""
        val storage = Storage()
        val server = Server()
        val before = NetkitController(storage,Engine(server)) { """{"version":1,"join":$primary}""" }
        before.restore()
        before.publish("Deliver after recovery","Chatter",false,"My alias")
        before.shutdown()
        server.offline = true
        val migrated = NetkitController(storage,Engine(server)) {
            """{"version":1,"join":$primary,"publicFeedReplica":{"id":"$replica","addrs":[]}}"""
        }
        migrated.restore()
        migrated.sync()
        assertEquals(1,migrated.state.value.pendingCount)
        assertTrue(server.attempts.isEmpty())
        assertTrue(migrated.state.value.snapshot.posts.any { it.body == "From backup" })
        assertTrue(migrated.state.value.snapshot.posts.any { it.pending && it.body == "Deliver after recovery" })
        assertEquals(setOf("type","replica"),server.replicaCommands.single().keys)
        assertFalse(migrated.state.value.directAvailable)
        migrated.blockAccount("opaque-event",true,"Anonymous neighbor")
        assertFalse(migrated.state.value.snapshot.posts.any { it.body == "From backup" })
        migrated.shutdown()

        val resumed = NetkitController(storage,Engine(server)) { error("Discovery unavailable") }
        resumed.restore()
        resumed.sync()
        assertTrue(resumed.state.value.snapshot.posts.none { it.body == "From backup" })
        server.offline=false
        resumed.sync()
        assertEquals(listOf("event-1"),server.attempts)
        assertEquals(0,resumed.state.value.pendingCount)
        assertTrue(resumed.state.value.online)
        assertTrue(resumed.state.value.snapshot.posts.any { !it.pending && it.body == "Deliver after recovery" })
        resumed.shutdown()
    }

    @Test fun offlineOutboxSurvivesRestartAndRetriesTheSameEventId() = runBlocking {
        val storage = Storage(); val server = Server().apply { offline = true }
        val first = NetkitController(storage, Engine(server))
        first.connect(join, "Neighbor")
        first.publish("Hello", "Chatter", true, "Private alias")
        assertEquals(1, first.state.value.pendingCount)
        assertEquals("Anonymous neighbor", first.state.value.snapshot.posts.first().author)
        assertTrue(first.state.value.snapshot.posts.first().pending)
        first.sync()
        assertEquals(1, first.state.value.pendingCount)
        assertTrue(server.attempts.isEmpty())
        first.shutdown()
        val second = NetkitController(storage, Engine(server))
        second.restore()
        assertEquals(1, second.state.value.pendingCount)
        server.offline = false; server.loseReceiptOnce = true
        second.sync()
        assertEquals(1, second.state.value.pendingCount)
        second.sync()
        assertEquals(0, second.state.value.pendingCount)
        assertEquals(listOf("event-1", "event-1"), server.attempts)
        assertEquals(1, server.accepted.size)
        assertFalse(second.state.value.snapshot.posts.first().pending)
        second.shutdown()
    }
    @Test fun coldBackgroundRefreshUsesSavedHomeAndClosesAfterOneBoundedBatch() = runBlocking {
        val storage = Storage()
        val server = Server()
        val first = NetkitController(storage, Engine(server))
        first.connect(join, "Neighbor")
        repeat(5) { first.publish("Queued $it", "Chatter", false, "Neighbor") }
        first.shutdown()

        val engine = Engine(server)
        val cold = NetkitController(storage, engine)
        cold.setForeground(false)
        server.loseReceiptOnce = true
        assertEquals(false, cold.backgroundRefresh(NearbyPreference()))
        assertEquals(5, cold.state.value.pendingCount)
        assertEquals(true, cold.backgroundRefresh(NearbyPreference()))
        assertEquals(2, cold.state.value.pendingCount)
        assertEquals(true, cold.backgroundRefresh(NearbyPreference()))
        assertEquals(0, cold.state.value.pendingCount)
        assertEquals(3, engine.closed.size)
        assertEquals(5, server.accepted.size)
        assertEquals(server.attempts[0], server.attempts[1])
    }

    @Test fun backgroundLaunchSkipsFreshProfilesAndDemoWithoutEnrolling() = runBlocking {
        val storage = Storage()
        val engine = Engine(Server())
        var enrollments = 0
        val cold = NetkitController(storage, engine) { enrollments++; error("Unexpected enrollment") }
        cold.setForeground(false)
        assertNull(cold.backgroundRefresh(NearbyPreference()))
        assertTrue(storage.secrets.isEmpty())
        cold.connect(join, "Neighbor")
        cold.useDemo()
        assertNull(cold.backgroundRefresh(NearbyPreference()))
        assertEquals(0, enrollments)
        assertEquals(1, engine.closed.size)
    }

    @Test fun aFailedDiskWriteNeverDeliversOrAcknowledgesTheNewPost() = runBlocking {
        val storage = Storage(); val server = Server(); val engine = Engine(server)
        val controller = NetkitController(storage, engine)
        controller.connect(join, "Neighbor")
        storage.failNextStateWrite = true
        assertFailsWith<IllegalStateException> { controller.publish("Keep the draft", "Friends", false, "Neighbor") }
        assertEquals(0, controller.state.value.pendingCount)
        assertTrue(server.accepted.isEmpty())
        val identity = Json.parseToJsonElement(storage.secrets.getValue("identity-v1")).jsonObject
        assertEquals(2, identity.getValue("nextSequence").jsonPrimitive.int)
        controller.shutdown()
    }
    @Test fun switchingCommunitiesCannotMisrouteTheOldOutbox() = runBlocking {
        val storage = Storage(); val server = Server(); val engine = Engine(server)
        val controller = NetkitController(storage, engine)
        controller.connect(join, "Neighbor")
        controller.publish("Only home A", "Plans", false, "Neighbor")
        assertFailsWith<IllegalStateException> { controller.connect("""{"home":"home-b"}""", "Neighbor") }
        assertEquals(1, controller.state.value.pendingCount)
        assertEquals(listOf(2L), engine.closed)
        assertTrue(storage.secrets.getValue("connection-v1").contains("home-a"))
        controller.shutdown()
    }
    @Test fun bookmarksAndLocalHidingSurviveRemoteRefreshes() = runBlocking {
        val storage = Storage(); val controller = NetkitController(storage, Engine(Server()))
        controller.connect(join, "Neighbor"); controller.publish("Bookmark", "Friends", true, "Neighbor"); controller.sync()
        val id = controller.state.value.snapshot.posts.first().id
        controller.save(id); controller.sync()
        assertTrue(controller.state.value.snapshot.posts.first().saved)
        assertEquals(id, controller.state.value.snapshot.posts.first().id)
        controller.hide(id); controller.sync()
        assertTrue(visiblePosts(controller.state.value.snapshot).isEmpty())
        controller.useDemo()
        assertFalse(controller.state.value.enabled)
        val restored = NetkitController(storage, Engine(Server()))
        restored.restore(); assertFalse(restored.state.value.enabled)
    }
    @Test fun communityCachesSurviveSwitchesAndProcessRestart() = runBlocking {
        val storage = Storage(); val server = Server(); val first = NetkitController(storage, Engine(server))
        first.connect(join, "Neighbor"); first.publish("Keep my bookmark", "Friends", true, "Neighbor"); first.sync()
        first.save(first.state.value.snapshot.posts.first().id)
        first.connect("""{"home":"home-b"}""", "Neighbor")
        assertTrue(first.state.value.snapshot.posts.isEmpty())
        first.shutdown()
        val restored = NetkitController(storage, Engine(server))
        restored.restore(); restored.connect(join, "Neighbor")
        assertTrue(restored.state.value.snapshot.posts.first().saved)
        restored.shutdown()
    }
    @Test fun failedConnectionPersistenceKeepsThePreviousCommunityAndHandle() = runBlocking {
        val storage = Storage(); val server = Server(); val engine = Engine(server)
        val controller = NetkitController(storage, engine)
        controller.connect(join, "Neighbor"); controller.publish("Still home A", "Friends", true, "Neighbor"); controller.sync()
        val before = controller.state.value.snapshot
        storage.failNextSecretWrite = true
        assertFailsWith<IllegalStateException> { controller.connect("""{"home":"home-b"}""", "Neighbor") }
        assertEquals(before, controller.state.value.snapshot)
        assertEquals(listOf(2L), engine.closed)
        storage.failNextStateWrite = true
        assertFailsWith<IllegalStateException> { controller.connect("""{"home":"home-b"}""", "Neighbor") }
        assertEquals(before, controller.state.value.snapshot)
        assertTrue(storage.secrets.getValue("connection-v1").contains("home-a"))
        controller.sync(); assertTrue(controller.state.value.online)
        controller.shutdown()
    }
    @Test fun savedGroupsAndCityRestoreWithoutPublishingCapabilitiesIntoTheFeedCache() = runBlocking {
        val storage = Storage(); val controller = NetkitController(storage, Engine(Server()))
        controller.connect(join, "Neighbor"); controller.refreshGroups()
        val cityGroup = controller.state.value.groups.first { it.id == "local-group" }
        controller.joinGroup(cityGroup)
        assertEquals("local-group", controller.state.value.activeGroup?.id)
        assertEquals("New York", controller.state.value.locality?.city)
        assertFalse(storage.data!!.contains("private-group-capability"))
        assertTrue(storage.secrets.getValue("groups-v1").contains("private-group-capability"))
        controller.useDemo()
        val restored = NetkitController(storage, Engine(Server()))
        restored.restore()
        assertFalse(restored.state.value.enabled)
        assertEquals("City readers", restored.state.value.joinedGroups.single().name)
        restored.openGroup("local-group")
        assertTrue(restored.state.value.enabled)
        assertEquals("local-group", restored.state.value.activeGroup?.id)
        assertTrue(restored.state.value.hasHomeFeed)
        restored.openHomeFeed()
        assertEquals(null, restored.state.value.activeGroup)
        assertEquals(1, restored.state.value.joinedGroups.size)
        restored.shutdown()
    }

    @Test fun groupSwitchGuardKeepsTheOutboxAndAllowsReopeningItsOwnGroup() = runBlocking {
        val controller = NetkitController(Storage(), Engine(Server()))
        controller.connect(join, "Neighbor"); controller.refreshGroups()
        val local = controller.state.value.groups.first { it.id == "local-group" }
        val worldwide = controller.state.value.groups.first { it.id == "world-group" }
        controller.joinGroup(local)
        controller.publish("Queued in this city", "Friends", true, "Neighbor")
        assertFailsWith<IllegalStateException> { controller.joinGroup(worldwide) }
        assertFailsWith<IllegalStateException> { controller.createGroup("New group", "A new group", "Friends", null) }
        assertEquals(1, controller.state.value.pendingCount)
        controller.useDemo(); controller.openGroup(local.id)
        assertEquals(1, controller.state.value.pendingCount)
        assertEquals(local.id, controller.state.value.activeGroup?.id)
        controller.shutdown()
    }

    @Test fun legacySingleCacheMigratesWithoutLosingItsSignedOutbox() = runBlocking {
        val storage = Storage(); val server = Server()
        val first = NetkitController(storage, Engine(server))
        first.connect(join, "Neighbor")
        first.publish("Queued before the archive wrapper", "Chatter", true, "Neighbor")
        first.shutdown()
        storage.data = Json.parseToJsonElement(storage.data!!).jsonObject.getValue("current").toString()

        val restored = NetkitController(storage, Engine(server))
        restored.restore()
        assertEquals(1, restored.state.value.pendingCount)
        restored.sync()
        assertEquals(listOf("event-1"), server.attempts)
        assertEquals(0, restored.state.value.pendingCount)
        assertTrue("current" in Json.parseToJsonElement(storage.data!!).jsonObject)
        restored.shutdown()
    }

    @Test fun switchingToDemoCompletesItsDurableTransitionDespiteCancellation() = runBlocking {
        val storage = Storage(); val engine = Engine(Server())
        val controller = NetkitController(storage, engine)
        controller.connect(join, "Neighbor")
        val started = CompletableDeferred<Unit>()
        val finish = CompletableDeferred<Unit>()
        engine.beforeClose = { started.complete(Unit); finish.await() }
        val switching = launch { controller.useDemo() }
        started.await()
        switching.cancel()
        finish.complete(Unit)
        switching.join()

        assertEquals(listOf(1L), engine.closed)
        assertFalse(controller.state.value.enabled)
        val restored = NetkitController(storage, Engine(Server()))
        restored.restore()
        assertFalse(restored.state.value.enabled)
    }

    @Test fun unreadableCacheFailsClosedAndDoesNotOverwritePendingData() = runBlocking {
        val storage = Storage().apply { data = "corrupt-but-not-disposable" }
        val controller = NetkitController(storage, Engine(Server()))
        assertFailsWith<IllegalStateException> { controller.connect(join, "Neighbor") }
        assertEquals("corrupt-but-not-disposable", storage.data)
    }
    @Test fun backgroundSyncDoesNotStartNewNetworkRequests() = runBlocking {
        val server = Server(); val controller = NetkitController(Storage(), Engine(server))
        controller.connect(join, "Neighbor"); controller.publish("Pending", "Chatter", true, "Neighbor")
        controller.setForeground(false); controller.sync()
        assertTrue(server.attempts.isEmpty()); assertEquals(1, controller.state.value.pendingCount)
        controller.setForeground(true); controller.sync()
        assertEquals(1, server.accepted.size)
        controller.shutdown()
    }
    @Test fun richOutboxRetainsProfileThroughRestartAndAnonymousPayloadLeavesItOut() = runBlocking {
        val storage = Storage(); val server = Server().apply { offline = true }
        val first = NetkitController(storage, Engine(server))
        first.connect(join, "Comet")
        val profile = CommunityProfile(pronouns = "they / them", bio = "My world", hereFor = listOf("Community"))
        first.publish("Bring a blanket", "Friends", false, "Comet", "Quiet company", profile)
        assertEquals(profile, first.state.value.snapshot.posts.first().profile)
        first.shutdown()
        val restored = NetkitController(storage, Engine(server)); restored.restore()
        server.offline = false; restored.sync()
        assertEquals("Quiet company", restored.state.value.snapshot.posts.first().headline)
        assertEquals(profile, restored.state.value.snapshot.posts.first().profile)
        restored.publish("A thought", "Chatter", true, "Comet", "Hello", profile)
        restored.sync()
        val anonymous = server.accepted.values.last().getValue("payload").jsonObject.getValue("action").jsonObject
        assertFalse(anonymous.containsKey("profile"))
        assertEquals("Anonymous neighbor", anonymous.getValue("displayName").jsonPrimitive.content)
        restored.shutdown()
    }
}
