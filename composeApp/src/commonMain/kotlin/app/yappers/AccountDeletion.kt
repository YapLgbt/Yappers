package app.yappers

import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.NonCancellable
import kotlinx.coroutines.withContext
import kotlinx.serialization.Serializable
import kotlinx.serialization.json.*

internal enum class DeletionStatus { Idle, Working, Retry, StorageLocked }

@Serializable
internal data class PendingDeletion(
    val join: JsonObject,
    val standby: JsonObject?,
    val wallet: JsonObject,
    val intent: JsonObject,
    val recoveryKey: String,
    val nextSequence: Long,
    val proofs: JsonArray = JsonArray(emptyList()),
)

/** All steps run under the controller's account mutex. Only the pinned Home sees signed proofs. */
internal class AccountDeletion(private val storage: NetkitStorage, private val engine: NetkitEngine) {
    suspend fun read(): JsonObject? = withContext(Dispatchers.Default) {
        storage.readSecret(ACCOUNT_DELETION_JOURNAL).takeUnless { it == "null" }?.let { Json.parseToJsonElement(it).jsonObject }
    }

    suspend fun persist(record: JsonObject) = withContext(NonCancellable + Dispatchers.Default) {
        storage.writeSecret(ACCOUNT_DELETION_JOURNAL, record.toString())
    }

    suspend fun prepare(handle: Long, join: JsonObject, standby: JsonObject?, wallet: JsonObject, sequence: Long): JsonObject {
        val intent = engine.request(handle, buildJsonObject {
            put("type", "prepareAccountDeletion"); put("wallet", wallet)
        }).getValue("intent").jsonObject
        val key = engine.portability(buildJsonObject {
            put("type", "recoveryKey"); put("wallet", wallet)
        }).getValue("key").jsonPrimitive.content
        val record = Json.encodeToJsonElement(PendingDeletion(join, standby, wallet, intent, key, sequence)).jsonObject
        return record
    }

    suspend fun acknowledgedKey(record: JsonObject): String =
        record["confirmedRecoveryKey"]?.jsonPrimitive?.content ?: send(Json.decodeFromJsonElement<PendingDeletion>(record))

    private suspend fun send(initial: PendingDeletion): String {
        var pending = initial
        val registration = pending.wallet.getValue("chain").jsonArray.last().jsonObject.getValue("payload").jsonObject
        val account = registration.getValue("account")
        val home = registration.getValue("home")
        check(initial.join.getValue("serverAddr").jsonObject.getValue("id") == home) { "Reconnect to the account’s home to delete it." }
        // Open the pinned original wallet directly: ordinary restore/registration may already be refused.
        val active = withContext(NonCancellable) {
            engine.open(buildJsonObject {
                put("join", pending.join); put("accountChain", pending.wallet.getValue("chain"))
                put("authorSecret", pending.wallet.getValue("deviceSecret")); put("nextSequence", pending.nextSequence)
                pending.standby?.let { put("fullHomeStandby", it) }
            }).getValue("handle").jsonPrimitive.long
        }
        try {
            while (true) {
                val result = engine.request(active, buildJsonObject {
                    put("type", "accountDeletionStep"); put("intent", pending.intent); put("proofs", pending.proofs)
                })
                if (result.getValue("deleted").jsonPrimitive.boolean) {
                    check(result["account"] == account && result["home"] == home) { "The deletion receipt did not match this account." }
                    persist(buildJsonObject { put("confirmedRecoveryKey", pending.recoveryKey) })
                    return pending.recoveryKey
                }
                val targets = result.getValue("targets").jsonArray
                check(targets.isNotEmpty()) { "Home did not confirm account deletion." }
                val batch = engine.request(active, buildJsonObject {
                    put("type", "prepareAccountDeletionBatch"); put("intent", pending.intent)
                    put("targets", targets); put("nextSequence", pending.nextSequence)
                })
                val proofs = batch.getValue("proofs").jsonArray
                val next = batch.getValue("nextSequence").jsonPrimitive.long
                check(proofs.size in 1..targets.size && next > pending.nextSequence && next - pending.nextSequence == proofs.size.toLong())
                pending = pending.copy(proofs = proofs, nextSequence = next)
                // Re-send these exact bytes after a timeout or process death; never re-sign the batch.
                persist(Json.encodeToJsonElement(pending).jsonObject)
            }
        } finally { withContext(NonCancellable) { engine.close(active) } }
    }
}
