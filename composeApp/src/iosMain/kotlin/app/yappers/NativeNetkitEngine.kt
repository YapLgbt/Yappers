@file:OptIn(kotlinx.cinterop.ExperimentalForeignApi::class)

package app.yappers

import app.yappers.netkit.ffi.*
import kotlinx.cinterop.*
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.launch
import kotlinx.coroutines.suspendCancellableCoroutine
import kotlinx.coroutines.withContext
import kotlinx.serialization.json.*
import kotlin.coroutines.AbstractCoroutineContextElement
import kotlin.coroutines.CoroutineContext
import kotlin.coroutines.resumeWithException

internal class RefreshScope(val id: ULong) : AbstractCoroutineContextElement(Key) {
    companion object Key : CoroutineContext.Key<RefreshScope>
}

// Independent of screen lifetimes: a cancelled open still needs to reclaim its native result.
// The Rust runtime also lives for the process; native resource cleanup must outlive callers.
private val nativeWorkers = CoroutineScope(SupervisorJob() + Dispatchers.Default)

/** The only C ownership boundary in Kotlin. Copy each result and free it exactly once. */
class NativeNetkitEngine : NetkitEngine {
    override val supportsPortability = true
    private fun unwrap(pointer: CPointer<ByteVar>?): JsonObject {
        checkNotNull(pointer) { "Netkit returned no result" }
        val value = try {
            Json.parseToJsonElement(pointer.toKString()).jsonObject
        } finally {
            yappers_netkit_string_free(pointer)
        }
        check(value.getValue("ok").jsonPrimitive.boolean) {
            value["error"]?.jsonPrimitive?.content ?: "Netkit request failed"
        }
        return value.getValue("data").jsonObject
    }

    override suspend fun portability(command: JsonObject): JsonObject = withContext(Dispatchers.Default) {
        memScoped { unwrap(yappers_netkit_portability(command.toString().cstr.ptr)) }
    }

    override suspend fun identity(): JsonObject = withContext(Dispatchers.Default) {
        unwrap(yappers_netkit_identity())
    }

    override suspend fun open(config: JsonObject): JsonObject = suspendCancellableCoroutine { continuation ->
        nativeWorkers.launch {
            try {
                val opened = memScoped { unwrap(yappers_netkit_open(config.toString().cstr.ptr)) }
                continuation.resume(opened) { _, resource, _ ->
                    // Cancellation before ownership transfer must not leak a newly bound endpoint.
                    nativeWorkers.launch { close(resource.getValue("handle").jsonPrimitive.long) }
                }
            } catch (error: Exception) {
                continuation.resumeWithException(error)
            }
        }
    }

    override suspend fun request(handle: Long, command: JsonObject): JsonObject = withContext(Dispatchers.Default) {
        val refresh = coroutineContext[RefreshScope]
        memScoped {
            val request = command.toString().cstr.ptr
            unwrap(if (refresh == null) yappers_netkit_request(handle.toULong(), request)
                   else yappers_netkit_refresh_request(refresh.id, handle.toULong(), request))
        }
    }

    override suspend fun close(handle: Long) {
        withContext(Dispatchers.Default) {
            unwrap(yappers_netkit_close(handle.toULong()))
        }
    }
}
