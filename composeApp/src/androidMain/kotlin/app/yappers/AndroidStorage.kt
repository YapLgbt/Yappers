package app.yappers

import android.content.Context
import android.security.keystore.KeyGenParameterSpec
import android.security.keystore.KeyProperties
import android.util.AtomicFile
import java.io.File
import java.io.FileNotFoundException
import java.security.KeyStore
import javax.crypto.Cipher
import javax.crypto.KeyGenerator
import javax.crypto.SecretKey
import javax.crypto.spec.GCMParameterSpec

/** One application-owned store for the foreground and WorkManager controllers. */
internal class AndroidStorage(context: Context) : NetkitStorage {
    private val dir = File(context.noBackupFilesDir, "account").apply { check(isDirectory || mkdirs()) }
    private val key: SecretKey by lazy {
        val alias = "yap-account-v1"
        val ks = KeyStore.getInstance("AndroidKeyStore").apply { load(null) }
        (ks.getKey(alias, null) as? SecretKey) ?: run {
            check(dir.listFiles()?.isEmpty() == true) { "Could not unlock the saved Yap account." }
            KeyGenerator.getInstance(KeyProperties.KEY_ALGORITHM_AES, "AndroidKeyStore").apply {
                init(KeyGenParameterSpec.Builder(alias, KeyProperties.PURPOSE_ENCRYPT or KeyProperties.PURPOSE_DECRYPT)
                    .setKeySize(256).setBlockModes(KeyProperties.BLOCK_MODE_GCM)
                    .setEncryptionPaddings(KeyProperties.ENCRYPTION_PADDING_NONE).build())
            }.generateKey()
        }
    }
    private fun file(name: String): AtomicFile {
        require(name.matches(Regex("[a-zA-Z0-9_.-]+")))
        return AtomicFile(File(dir, name))
    }
    @Synchronized private fun read(name: String): String? {
        val record = try { file(name).openRead().use { it.readBytes() } }
        catch (e: FileNotFoundException) { if (File(dir,name).exists()) throw e else return null }
        require(record.size >= 28)
        val cipher = Cipher.getInstance("AES/GCM/NoPadding")
        cipher.init(Cipher.DECRYPT_MODE, key, GCMParameterSpec(128, record.copyOfRange(0,12)))
        cipher.updateAAD(name.encodeToByteArray())
        return cipher.doFinal(record,12,record.size-12).decodeToString()
    }
    @Synchronized private fun write(name: String, value: String) {
        val cipher = Cipher.getInstance("AES/GCM/NoPadding")
        cipher.init(Cipher.ENCRYPT_MODE, key)
        cipher.updateAAD(name.encodeToByteArray())
        val ciphertext = cipher.doFinal(value.encodeToByteArray())
        val file = file(name)
        val stream = file.startWrite()
        try {
            stream.write(cipher.iv); stream.write(ciphertext); stream.fd.sync()
            file.finishWrite(stream)
        } catch (e: Throwable) { file.failWrite(stream); throw e }
    }

    override fun readState() = read("state") ?: "null"
    override fun writeState(value: String) = write("state", value)
    override fun readSecret(key: String) = read("secret-$key") ?: "null"
    override fun writeSecret(key: String, value: String) = write("secret-$key", value)
    val local = object : LocalStorage {
        override fun read() = read("local")
        override fun write(value: String) = write("local", value)
    }
}
