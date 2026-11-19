package app.yappers

import com.sun.jna.Library
import com.sun.jna.Native
import com.sun.jna.Pointer
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.withContext
import kotlinx.serialization.json.*
import java.nio.file.Files
import java.nio.file.attribute.PosixFilePermissions

/** Owns Rust-returned pointers until copied and freed; blocking Iroh work never runs on Swing. */
internal class DesktopNetkitEngine : NetkitEngine {
    override val supportsPortability = true
    private interface Abi : Library {
        fun yappers_netkit_identity(): Pointer?
        fun yappers_netkit_portability(command: String): Pointer?
        fun yappers_netkit_open(config: String): Pointer?
        fun yappers_netkit_request(handle: Long, request: String): Pointer?
        fun yappers_netkit_close(handle: Long): Pointer?
        fun yappers_netkit_string_free(value: Pointer)
    }
    private val abi: Abi by lazy {
        val directory = Files.createTempDirectory("yappers-netkit-", PosixFilePermissions.asFileAttribute(PosixFilePermissions.fromString("rwx------")))
        val library = directory.resolve("libyappers_netkit.dylib")
        checkNotNull(javaClass.getResourceAsStream("/netkit/libyappers_netkit.dylib")) { "Rust library missing. Run scripts/run-desktop.sh." }
            .use { Files.copy(it, library) }
        directory.toFile().deleteOnExit()
        library.toFile().deleteOnExit()
        Native.load(library.toString(), Abi::class.java, mapOf(Library.OPTION_STRING_ENCODING to "UTF-8"))
    }
    private suspend fun call(operation: (Abi) -> Pointer?): JsonObject = withContext(Dispatchers.IO) {
        val native = abi
        val pointer = checkNotNull(operation(native)) { "Netkit returned an empty response." }
        val response = try { Json.parseToJsonElement(pointer.getString(0, "UTF-8")).jsonObject }
        finally { native.yappers_netkit_string_free(pointer) }
        check(response["ok"]?.jsonPrimitive?.boolean == true) {
            response["error"]?.jsonPrimitive?.content ?: "Netkit request failed."
        }
        response.getValue("data").jsonObject
    }
    override suspend fun portability(command: JsonObject) = call { it.yappers_netkit_portability(command.toString()) }
    override suspend fun identity() = call { it.yappers_netkit_identity() }
    override suspend fun open(config: JsonObject) = call { it.yappers_netkit_open(config.toString()) }
    override suspend fun request(handle: Long, command: JsonObject) = call { it.yappers_netkit_request(handle, command.toString()) }
    override suspend fun close(handle: Long) { call { it.yappers_netkit_close(handle) } }
}
