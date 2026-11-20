package app.yappers

import java.nio.channels.FileChannel
import java.nio.file.*
import java.nio.file.attribute.PosixFilePermissions

/** Development-only desktop store. Credentials are owner-readable files, not iOS Keychain.
 * Atomic replace and fsync preserve the controller's durable outbox/sequence ordering.
 * A lifetime lock prevents two desktop processes from reusing one signing sequence.
 */
internal class DesktopStorage(val directory: Path) : NetkitStorage, AutoCloseable {
    private val channel: FileChannel
    private val lock: java.nio.channels.FileLock
    init {
        Files.createDirectories(directory, PosixFilePermissions.asFileAttribute(PosixFilePermissions.fromString("rwx------")))
        Files.setPosixFilePermissions(directory, PosixFilePermissions.fromString("rwx------"))
        channel = FileChannel.open(directory.resolve("session.lock"), StandardOpenOption.CREATE, StandardOpenOption.WRITE)
        lock = try { checkNotNull(channel.tryLock()) { "This desktop data directory is already open. Use --data-dir for another identity." } }
        catch (failure: Exception) { channel.close(); throw failure }
    }
    @Synchronized private fun readFile(name: String): String? {
        val path = directory.resolve(name)
        return if (Files.notExists(path)) null else Files.readString(path)
    }
    @Synchronized private fun writeFile(name: String, value: String) {
        val target = directory.resolve(name)
        val temporary = Files.createTempFile(directory, ".write-", ".tmp",
            PosixFilePermissions.asFileAttribute(PosixFilePermissions.fromString("rw-------")))
        try {
            FileChannel.open(temporary, StandardOpenOption.WRITE).use {
                val buffer = java.nio.ByteBuffer.wrap(value.toByteArray(Charsets.UTF_8))
                while (buffer.hasRemaining()) it.write(buffer)
                it.force(true)
            }
            Files.move(temporary, target, StandardCopyOption.ATOMIC_MOVE, StandardCopyOption.REPLACE_EXISTING)
            FileChannel.open(directory, StandardOpenOption.READ).use { it.force(true) }
        } finally { Files.deleteIfExists(temporary) }
    }
    override fun readState() = readFile("network.json") ?: "null"
    override fun writeState(value: String) = writeFile("network.json", value)
    private fun secretName(key: String): String {
        require(key.matches(Regex("[a-z0-9-]+")))
        return "$key.json"
    }
    override fun readSecret(key: String) = readFile(secretName(key)) ?: "null"
    override fun writeSecret(key: String, value: String) = writeFile(secretName(key), value)
    @Synchronized override fun eraseDeletedAccount(recoveryKey: String) {
        val owned = setOf("network.json", "demo.json", "identity-v1.json", "connection-v1.json",
            "public-feed-route-v1.json", "groups-v1.json", "welcome-backup-v1.json", "welcome-device-v1.json")
        val messages = Regex("""(?:direct-v1(?:-[0-9a-f]{64}){3}|private-groups-v1(?:-[0-9a-f]{64}){2})\.json""")
        Files.newDirectoryStream(directory).use { entries ->
            for (entry in entries) {
                val name = entry.fileName.toString()
                if (name in owned || messages.matches(name) || (name.startsWith(".write-") && name.endsWith(".tmp")))
                    Files.delete(entry)
            }
        }
        FileChannel.open(directory, StandardOpenOption.READ).use {
            it.force(true)
            Files.deleteIfExists(directory.resolve(secretName(ACCOUNT_DELETION_JOURNAL)))
            it.force(true)
        }
    }
    val local = object : LocalStorage {
        override fun read() = readFile("demo.json")
        override fun write(value: String) = writeFile("demo.json", value)
    }
    override fun close() { lock.release(); channel.close() }
}
