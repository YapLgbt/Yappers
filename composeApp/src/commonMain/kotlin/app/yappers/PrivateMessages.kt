package app.yappers

import kotlinx.coroutines.CancellationException
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.NonCancellable
import kotlinx.coroutines.withContext
import kotlinx.serialization.Serializable
import kotlinx.serialization.json.*

@Serializable
data class DirectMessage(
    val id: String, val peer: String, val label: String, val body: String,
    val outgoing: Boolean, val status: String, val age: Int = 0, val timestamp: Long = 0
)
@Serializable
data class MessageContact(val peer: String, val label: String)
@Serializable
data class BlockedAccount(val id: String, val label: String)

internal const val OTHER_MESSAGING_DEVICE = "This account’s inbox is linked to another installation of Yap. Open it there to read or send messages. Account recovery doesn’t transfer inbox keys."

// Stored alongside the device-private Olm state, never sent to the engine or exported.
private const val LOCAL_REGISTRATION = "localRegistration"
internal val JsonObject.mailboxElsewhere get() = this[LOCAL_REGISTRATION]?.jsonPrimitive?.content == "elsewhere"
internal val JsonObject.canUseMailbox get() = !mailboxElsewhere && this[LOCAL_REGISTRATION]?.jsonPrimitive?.content != "pending"

/** Called under the controller mutex. Keep keys, ratchets, pending packets and history in one
 * atomic device-private item, separate from the feed cache and portable profile backups. */
internal class PrivateMessages(private val storage: NetkitStorage, private val engine: NetkitEngine) {
    suspend fun read(key: String): JsonObject? = withContext(Dispatchers.Default) {
        val saved = storage.readSecret(key)
        if (saved == "null") null else Json.parseToJsonElement(saved).jsonObject
    }
    suspend fun step(handle: Long, key: String, action: JsonObject): JsonObject {
        val old = read(key)
        suspend fun advance(previous: JsonObject?, operation: JsonObject): JsonObject {
            val kind = operation.getValue("kind").jsonPrimitive.content
            val saved = previous?.let { JsonObject(it - LOCAL_REGISTRATION) }
            val next = try {
                val result = engine.request(handle, buildJsonObject {
                    put("type", "direct")
                    put("state", saved ?: JsonNull)
                    put("action", operation)
                }).getValue("state").jsonObject
                val registration = when (kind) {
                    "initialize" -> JsonPrimitive("pending")
                    "sync" -> JsonPrimitive("ready")
                    else -> previous?.get(LOCAL_REGISTRATION)
                }
                if (registration == null) result else JsonObject(result + (LOCAL_REGISTRATION to registration))
            } catch (failure: Exception) {
                if (failure is CancellationException || kind != "sync" || previous == null ||
                    failure.message?.substringAfterLast(": ") != "messaging is registered to a different device") throw failure
                JsonObject(previous + (LOCAL_REGISTRATION to JsonPrimitive("elsewhere")))
            }
            if (previous != next) withContext(NonCancellable + Dispatchers.Default) {
                storage.writeSecret(key, next.toString())
            }
            return next
        }
        val current = old ?: advance(null, buildJsonObject { put("kind", "initialize") })
        return advance(current, action)
    }
}
