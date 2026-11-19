package app.yappers

import android.content.Context
import kotlinx.coroutines.*
import kotlinx.serialization.json.*
import kotlin.coroutines.AbstractCoroutineContextElement
import kotlin.coroutines.CoroutineContext
import kotlin.coroutines.resumeWithException

internal object AndroidAbi {
    init { System.loadLibrary("yap_android") }
    external fun initialize(application: Context)
    external fun beginRefresh(): Long
    external fun endRefresh(token: Long)
    external fun call(operation: Int, handle: Long, token: Long, input: ByteArray?): ByteArray?
}

internal class AndroidRefreshToken(val value: Long) : AbstractCoroutineContextElement(Key) {
    companion object Key : CoroutineContext.Key<AndroidRefreshToken>
}

internal class AndroidNetkitEngine : NetkitEngine {
    override val supportsPortability = true
    private val workers = CoroutineScope(SupervisorJob() + Dispatchers.IO)

    private suspend fun execute(op: Int, input: JsonObject? = null, handle: Long = 0): JsonObject =
        withContext(Dispatchers.IO) {
            val bytes = checkNotNull(AndroidAbi.call(op, handle, coroutineContext[AndroidRefreshToken]?.value ?: 0,
                input?.toString()?.encodeToByteArray())) { "The native client could not complete this request." }
            val response = Json.parseToJsonElement(bytes.decodeToString()).jsonObject
            check(response.getValue("ok").jsonPrimitive.boolean) {
                response["error"]?.jsonPrimitive?.content ?: "Netkit request failed"
            }
            response.getValue("data").jsonObject
        }

    override suspend fun identity() = execute(0)
    override suspend fun portability(command: JsonObject) = execute(1, command)
    override suspend fun open(config: JsonObject): JsonObject = suspendCancellableCoroutine { waiter ->
        workers.launch {
            try {
                val result = execute(2, config)
                waiter.resume(result) { _, owned, _ -> workers.launch { close(owned.getValue("handle").jsonPrimitive.long) } }
            } catch (failure: Exception) { waiter.resumeWithException(failure) }
        }
    }
    override suspend fun request(handle: Long, command: JsonObject) = execute(3, command, handle)
    override suspend fun close(handle: Long) { withContext(NonCancellable) { execute(4, handle = handle) } }
}
