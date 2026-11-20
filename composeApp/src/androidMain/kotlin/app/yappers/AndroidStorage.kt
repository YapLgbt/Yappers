package app.yappers

import android.content.Context
import android.security.keystore.KeyGenParameterSpec
import android.security.keystore.KeyProperties
import android.system.Os
import android.system.OsConstants
import android.util.AtomicFile
import androidx.work.WorkManager
import java.io.File
import java.io.FileNotFoundException
import java.security.KeyStore
import javax.crypto.Cipher
import javax.crypto.KeyGenerator
import javax.crypto.SecretKey
import javax.crypto.spec.GCMParameterSpec

/** One application-owned store for the foreground and WorkManager controllers. */
internal class AndroidStorage(context: Context) : NetkitStorage {
    private val app = context.applicationContext
    private val dir = File(app.noBackupFilesDir, "account").apply { check(isDirectory || mkdirs()) }
    private val journal = File(app.noBackupFilesDir, ACCOUNT_DELETION_JOURNAL)
    private val retired = File(app.noBackupFilesDir, "$ACCOUNT_DELETION_JOURNAL.retired")
    private val journalRecord = "secret-$ACCOUNT_DELETION_JOURNAL"
    private val accountAlias = "yap-account-v1"
    private val journalAlias = "yap-account-deletion-journal-v1"
    private var accountKey = lazy { loadKey("state") }
    private fun key(name: String) = if (name == journalRecord) loadKey(name) else accountKey.value
    private fun AtomicFile.hasRecord() = baseFile.exists() || File(baseFile.path + ".bak").exists()
    private fun syncDirectory(directory: File) {
        val fd = Os.open(directory.path, OsConstants.O_RDONLY, 0)
        try { Os.fsync(fd) } finally { Os.close(fd) }
    }
    private fun finishRetirement() {
        if (!retired.exists()) return
        AtomicFile(journal).delete()
        check(!AtomicFile(journal).hasRecord() && !File(journal.path + ".new").exists())
        KeyStore.getInstance("AndroidKeyStore").apply { load(null); deleteEntry(journalAlias) }
        check(retired.delete()) { "Could not clear account deletion journal." }
        syncDirectory(app.noBackupFilesDir)
    }
    private fun loadKey(name: String): SecretKey {
        val alias = if (name == journalRecord) journalAlias else accountAlias
        val ks = KeyStore.getInstance("AndroidKeyStore").apply { load(null) }
        return (ks.getKey(alias, null) as? SecretKey) ?: run {
            val empty = if (name == journalRecord) !AtomicFile(journal).hasRecord() else dir.listFiles()?.isEmpty() == true
            check(empty) { "Could not unlock the saved Yap account." }
            KeyGenerator.getInstance(KeyProperties.KEY_ALGORITHM_AES, "AndroidKeyStore").apply {
                init(KeyGenParameterSpec.Builder(alias, KeyProperties.PURPOSE_ENCRYPT or KeyProperties.PURPOSE_DECRYPT)
                    .setKeySize(256).setBlockModes(KeyProperties.BLOCK_MODE_GCM)
                    .setEncryptionPaddings(KeyProperties.ENCRYPTION_PADDING_NONE).build())
            }.generateKey()
        }
    }
    private fun file(name: String): AtomicFile {
        require(name.matches(Regex("[a-zA-Z0-9_.-]+")))
        return AtomicFile(if (name == journalRecord) journal else File(dir, name))
    }
    @Synchronized private fun read(name: String): String? {
        val record = try { file(name).openRead().use { it.readBytes() } }
        catch (e: FileNotFoundException) { if (file(name).hasRecord()) throw e else return null }
        require(record.size >= 28)
        val cipher = Cipher.getInstance("AES/GCM/NoPadding")
        cipher.init(Cipher.DECRYPT_MODE, key(name), GCMParameterSpec(128, record.copyOfRange(0,12)))
        cipher.updateAAD(name.encodeToByteArray())
        return cipher.doFinal(record,12,record.size-12).decodeToString()
    }
    @Synchronized private fun write(name: String, value: String) {
        val cipher = Cipher.getInstance("AES/GCM/NoPadding")
        cipher.init(Cipher.ENCRYPT_MODE, key(name))
        cipher.updateAAD(name.encodeToByteArray())
        val ciphertext = cipher.doFinal(value.encodeToByteArray())
        val file = file(name)
        val stream = file.startWrite()
        try {
            stream.write(cipher.iv); stream.write(ciphertext); stream.fd.sync()
            file.finishWrite(stream)
        } catch (e: Throwable) { file.failWrite(stream); throw e }
        if (name == journalRecord) {
            // AtomicFile logs rename failures. Don't start an RPC unless the
            // exact signed batch is readable at the committed path.
            check(read(name) == value) { "Could not save account deletion progress." }
            syncDirectory(app.noBackupFilesDir)
        }
    }

    override fun readState() = read("state") ?: "null"
    override fun writeState(value: String) = write("state", value)
    @Synchronized override fun readSecret(key: String): String {
        if (key == ACCOUNT_DELETION_JOURNAL) finishRetirement()
        return read("secret-$key") ?: "null"
    }
    override fun writeSecret(key: String, value: String) = write("secret-$key", value)
    @Synchronized override fun eraseDeletedAccount(recoveryKey: String) {
        val work = WorkManager.getInstance(app)
        work.cancelUniqueWork("yap-refresh").result.get()
        work.cancelUniqueWork("yap-delivery").result.get()
        AndroidPush.clearAccount(app)
        check(app.getSharedPreferences("installation", Context.MODE_PRIVATE).edit().remove("welcomed").commit())
        for (record in checkNotNull(dir.listFiles())) {
            check(record.delete()) { "Could not remove an account file." }
        }
        syncDirectory(dir)
        val ks = KeyStore.getInstance("AndroidKeyStore").apply { load(null) }
        ks.deleteEntry(accountAlias)
        accountKey = lazy { loadKey("state") }
        // The independent journal key lets retry read the confirmed phase even
        // after the account encryption key has already been destroyed.
        if (journal.exists()) {
            Os.rename(journal.path, retired.path)
            syncDirectory(app.noBackupFilesDir)
        }
        finishRetirement()
    }
    val local = object : LocalStorage {
        override fun read() = read("local")
        override fun write(value: String) = write("local", value)
    }
}
