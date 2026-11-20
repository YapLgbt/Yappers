package app.yappers

import java.nio.file.DirectoryNotEmptyException
import java.nio.file.Files
import kotlin.test.*

class DeletedAccountStorageTest {
    @Test fun confirmedDeletionResumesAfterPartialCleanupAndPreservesUnrelatedFiles() {
        val dir = Files.createTempDirectory("yap-delete-test")
        try {
            val device = "a".repeat(64)
            val direct = "direct-v1-$device-$device-$device"
            val group = "private-groups-v1-$device-$device"
            val confirmed = """{"confirmedRecoveryKey":"test-key"}"""
            DesktopStorage(dir).use { store ->
                store.writeState("drafts")
                store.local.write("local profile")
                for (key in listOf(direct, group, "welcome-device-v1", "groups-v1")) store.writeSecret(key, "private")
                store.writeSecret(ACCOUNT_DELETION_JOURNAL, confirmed)
                Files.writeString(dir.resolve("unrelated.json"), "keep")
                Files.writeString(dir.resolve("Yap-Recovery.age"), "user export")
                // Simulate an unlink failure before the final journal removal.
                val blocker = Files.createDirectory(dir.resolve("identity-v1.json")).resolve("blocker")
                Files.writeString(blocker, "test")
                assertFailsWith<DirectoryNotEmptyException> { store.eraseDeletedAccount("test-key") }
                assertEquals(confirmed, store.readSecret(ACCOUNT_DELETION_JOURNAL))
                Files.delete(blocker)
            }
            DesktopStorage(dir).use { store ->
                assertEquals(confirmed, store.readSecret(ACCOUNT_DELETION_JOURNAL))
                repeat(2) { store.eraseDeletedAccount("test-key") }
                assertNull(store.local.read())
                assertEquals("null", store.readState())
                assertEquals("null", store.readSecret(ACCOUNT_DELETION_JOURNAL))
                assertEquals(setOf("session.lock", "unrelated.json", "Yap-Recovery.age"),
                    Files.newDirectoryStream(dir).use { files -> files.map { it.fileName.toString() }.toSet() })
                store.writeSecret("identity-v1", "new identity")
                assertEquals("new identity", store.readSecret("identity-v1"))
            }
        } finally { dir.toFile().deleteRecursively() }
    }
}
