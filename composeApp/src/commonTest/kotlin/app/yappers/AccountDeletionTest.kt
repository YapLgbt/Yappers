package app.yappers

import kotlinx.coroutines.CompletableDeferred
import kotlinx.coroutines.CoroutineStart
import kotlinx.coroutines.async
import kotlinx.coroutines.runBlocking
import kotlinx.serialization.json.*
import kotlin.test.*

class AccountDeletionTest {
    private fun obj(text: String) = Json.parseToJsonElement(text).jsonObject
    private val wallet = obj("""{"deviceSecret":"device","chain":[{"payload":{"account":"owner","home":"home"}}],"nextSequence":17}""")
    private val join = obj("""{"serverAddr":{"id":"home"},"community":"home/public","accessToken":"membership"}""")

    private inner class Disk : NetkitStorage {
        var data = "null"
        val secrets = mutableMapOf(
            "identity-v1" to buildJsonObject { put("secret","device"); put("wallet",wallet); put("nextSequence",17) }.toString(),
            "connection-v1" to buildJsonObject { put("enabled",true); put("join",join) }.toString(),
            "private-draft" to "draft",
        )
        var wipeAttempts = 0
        var failCleanup = false
        override fun readState() = data
        override fun writeState(value: String) { data = value }
        override fun readSecret(key: String) = secrets[key] ?: "null"
        override fun writeSecret(key: String, value: String) { secrets[key] = value }
        override fun eraseDeletedAccount(recoveryKey: String) {
            assertEquals("recovery", recoveryKey)
            assertEquals("recovery", obj(readSecret(ACCOUNT_DELETION_JOURNAL)).getValue("confirmedRecoveryKey").jsonPrimitive.content)
            wipeAttempts++
            if (failCleanup) error("Locked")
            data = "null"
            secrets.clear()
        }
    }

    private inner class Engine(val disk: Disk) : NetkitEngine {
        val intent = obj("""{"account":"owner","signed":"original-intent"}""")
        val remaining = mutableListOf("post","reply")
        val prepared = mutableListOf<JsonObject>()
        val sent = mutableListOf<JsonArray>()
        var probeAvailable = true
        var losePage = false
        var loseAck = false
        var wrongAccount = false
        var terminalEntered: CompletableDeferred<Unit>? = null
        var releaseTerminal: CompletableDeferred<Unit>? = null
        var opens = 0
        override val supportsPortability = true
        override suspend fun identity(): JsonObject = error("Deletion must never create an identity")
        override suspend fun portability(command: JsonObject): JsonObject {
            assertEquals("recoveryKey",command.getValue("type").jsonPrimitive.content)
            return obj("""{"key":"recovery"}""")
        }
        override suspend fun open(config: JsonObject): JsonObject {
            assertEquals(join,config["join"])
            assertEquals(wallet["chain"],config["accountChain"])
            return buildJsonObject { put("handle",++opens); put("home","home"); put("community","home/public"); put("author","device") }
        }
        override suspend fun close(handle: Long) {}
        override suspend fun request(handle: Long, command: JsonObject): JsonObject = when (command.getValue("type").jsonPrimitive.content) {
            "prepareAccountDeletion" -> {
                check(probeAvailable) { "Old home" }
                assertEquals("null",disk.readSecret(ACCOUNT_DELETION_JOURNAL))
                buildJsonObject { put("intent",intent) }
            }
            "prepareAccountDeletionBatch" -> {
                val target = command.getValue("targets").jsonArray.first().jsonObject
                val n = command.getValue("nextSequence").jsonPrimitive.long
                // Return only the first target, as a frame-limited Rust client may do.
                val proof = buildJsonObject { put("target",target.getValue("id")); put("sequence",n) }
                prepared += proof
                buildJsonObject { put("proofs",JsonArray(listOf(proof))); put("nextSequence",n+1) }
            }
            "accountDeletionStep" -> {
                val proofs = command.getValue("proofs").jsonArray
                val journal = Json.decodeFromJsonElement<PendingDeletion>(obj(disk.readSecret(ACCOUNT_DELETION_JOURNAL)))
                assertEquals(intent,command["intent"])
                assertEquals(intent,journal.intent)
                assertEquals(proofs,journal.proofs)
                sent.add(proofs)
                for (proof in proofs) remaining.remove(proof.jsonObject.getValue("target").jsonPrimitive.content)
                if (losePage && proofs.isNotEmpty()) { losePage = false; error("Page response lost") }
                if (remaining.isEmpty()) {
                    terminalEntered?.complete(Unit)
                    releaseTerminal?.await()
                    if (loseAck) { loseAck = false; error("Terminal response lost") }
                    buildJsonObject { put("deleted",true); put("account",if (wrongAccount) "other" else "owner"); put("home","home") }
                } else buildJsonObject {
                    put("deleted",false)
                    put("targets",buildJsonArray { for (id in remaining) add(buildJsonObject { put("id",id); put("community","scope-$id") }) })
                }
            }
            else -> error("Unexpected command during deletion")
        }
    }

    @Test fun refusedPreflightKeepsSettingsUsableAndCredentialsIntact() = runBlocking {
        val disk = Disk()
        val engine = Engine(disk).apply { probeAvailable = false }
        val controller = NetkitController(disk,engine)
        controller.restore()
        assertFails { controller.deleteAccount(true) }
        assertEquals(DeletionStatus.Idle,controller.deletion.value)
        assertEquals("null",disk.readSecret(ACCOUNT_DELETION_JOURNAL))
        assertTrue(controller.state.value.enabled && controller.hasOpenConnection())
        assertEquals("draft", disk.readSecret("private-draft"))
        assertTrue(engine.sent.isEmpty())
        assertEquals(0,disk.wipeAttempts)
    }

    @Test fun queuedBackgroundRefreshAndStaleViewsCannotRepopulateAfterErasure() = runBlocking {
        val disk = Disk()
        val engine = Engine(disk).apply {
            remaining.clear()
            terminalEntered = CompletableDeferred()
            releaseTerminal = CompletableDeferred()
        }
        val controller = NetkitController(disk,engine)
        controller.restore()
        var localWrites = 0
        val oldView = AppContentState(object : LocalStorage {
            override fun read(): String? = null
            override fun write(value: String) { localWrites++ }
        }, controller)
        val deletion = async { controller.deleteAccount(true) }
        engine.terminalEntered!!.await()
        val background = async(start = CoroutineStart.UNDISPATCHED) { controller.backgroundRefresh(NearbyPreference()) }
        val staleAccountWrite = async(start = CoroutineStart.UNDISPATCHED) { runCatching { controller.useDemo() } }
        engine.releaseTerminal!!.complete(Unit)
        deletion.await()
        assertNull(background.await())
        assertTrue(staleAccountWrite.await().isFailure)
        assertFails { oldView.repository.publish("old draft", "Plans", false) }
        assertEquals(0,localWrites)
        assertTrue(disk.secrets.isEmpty())
        assertEquals("null",disk.data)
        assertTrue(controller.welcome.value)
    }

    @Test fun lostRepliesReplayDurablePrefixesAndCleanupResumesWithoutRegistering() = runBlocking {
        val disk = Disk()
        val engine = Engine(disk).apply { losePage = true; loseAck = true }
        val original = NetkitController(disk,engine)
        original.restore()
        assertFails { original.deleteAccount(true) }
        assertEquals(DeletionStatus.Retry,original.deletion.value)
        val reopened = NetkitController(disk,engine)
        assertEquals(DeletionStatus.Retry,reopened.deletion.value)
        val opens = engine.opens
        reopened.restore()
        assertNull(reopened.backgroundRefresh(NearbyPreference()))
        assertEquals(opens,engine.opens)
        assertFails { reopened.useDemo() }

        assertFails { reopened.deleteAccount(false) }
        assertTrue(engine.remaining.isEmpty())
        assertEquals(engine.sent[1],engine.sent[2], "lost page must replay the same proofs")
        assertEquals(listOf(17L,18L),engine.prepared.map { it.getValue("sequence").jsonPrimitive.long })
        assertEquals("draft",disk.readSecret("private-draft"))
        assertEquals(0,disk.wipeAttempts)

        engine.wrongAccount = true
        assertFails { reopened.deleteAccount(false) }
        assertEquals(0,disk.wipeAttempts)
        engine.wrongAccount = false
        disk.failCleanup = true
        assertFails { reopened.deleteAccount(false) }
        assertEquals(engine.sent[3],engine.sent.last(), "lost terminal receipt must replay the same proofs")
        val requests = engine.sent.size
        val beforeCleanup = engine.opens
        val final = NetkitController(disk,engine)
        disk.failCleanup = false
        final.deleteAccount(false)
        assertEquals(beforeCleanup,engine.opens)
        assertEquals(requests,engine.sent.size)
        assertEquals(2,disk.wipeAttempts)
        assertTrue(disk.secrets.isEmpty())
        assertTrue(final.welcome.value)
        assertFalse(final.state.value.enabled)
        assertTrue(final.state.value.snapshot.posts.isEmpty())
        assertNull(final.backgroundRefresh(NearbyPreference()))
        final.restore()
        assertEquals(beforeCleanup,engine.opens)
    }
}
