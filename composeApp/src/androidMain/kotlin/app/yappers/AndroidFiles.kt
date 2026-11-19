package app.yappers

import android.graphics.Bitmap
import android.graphics.ImageDecoder
import android.net.Uri
import android.provider.OpenableColumns
import androidx.activity.result.PickVisualMediaRequest
import androidx.activity.ComponentActivity
import androidx.activity.result.contract.ActivityResultContracts
import androidx.credentials.*
import androidx.credentials.exceptions.*
import androidx.lifecycle.lifecycleScope
import java.io.ByteArrayOutputStream
import kotlinx.coroutines.*
import kotlin.io.encoding.Base64
import kotlin.math.roundToInt

private typealias FileResult = (String?, String?) -> Unit

internal class AndroidFiles(private val activity: ComponentActivity) : ProfilePhotoLibrary, RecoveryFiles, PasswordManagerVault {
    private val resolver get() = activity.contentResolver
    private var photo: FileResult? = null
    private var load: FileResult? = null
    private var save: Pair<String, FileResult>? = null
    private val credentials by lazy { CredentialManager.create(activity) }

    private fun readResult(uri: Uri?, callback: FileResult?, operation: (Uri) -> String) {
        if (callback == null) return
        if (uri == null) { callback(null, "Canceled."); return }
        activity.lifecycleScope.launch {
            try { callback(withContext(Dispatchers.IO) { operation(uri) }, null) }
            catch (e: CancellationException) { throw e }
            catch (e: Exception) { callback(null, e.localizedMessage ?: "Couldn’t open this file.") }
        }
    }

    private val pickPhoto = activity.registerForActivityResult(ActivityResultContracts.PickVisualMedia()) { uri ->
        val callback = photo
        photo = null
        if (uri == null) { callback?.invoke(null, null); return@registerForActivityResult }
        readResult(uri, callback) { selected ->
            val bitmap = ImageDecoder.decodeBitmap(ImageDecoder.createSource(resolver, selected)) { decoder, info, _ ->
                val size = info.size
                val scale = minOf(1.0, 1536.0 / maxOf(size.width, size.height))
                decoder.setTargetSize(maxOf(1,(size.width * scale).roundToInt()), maxOf(1,(size.height * scale).roundToInt()))
                decoder.allocator = ImageDecoder.ALLOCATOR_SOFTWARE
            }
            try {
                val output = ByteArrayOutputStream()
                check(bitmap.compress(Bitmap.CompressFormat.JPEG, 88, output))
                Base64.Default.encode(output.toByteArray())
            } finally { bitmap.recycle() }
        }
    }
    private val open = activity.registerForActivityResult(ActivityResultContracts.OpenDocument()) { uri ->
        val callback = load
        load = null
        readResult(uri, callback) { selected ->
            val output = ByteArrayOutputStream()
            checkNotNull(resolver.openInputStream(selected)).use { stream ->
                val buffer = ByteArray(8192)
                while (true) {
                    val n = stream.read(buffer)
                    if (n < 0) break
                    require(output.size() + n <= 16 * 1024 * 1024 + 65536) { "This file is too large to be a Yap backup." }
                    output.write(buffer, 0, n)
                }
            }
            output.toByteArray().toHexString()
        }
    }
    private val create = activity.registerForActivityResult(ActivityResultContracts.CreateDocument("application/octet-stream")) { uri ->
        val pending = save
        save = null
        if (pending != null) readResult(uri, pending.second) { target ->
            checkNotNull(resolver.openOutputStream(target, "wt")).use { it.write(pending.first.hexToByteArray()) }
            resolver.query(target, arrayOf(OpenableColumns.DISPLAY_NAME), null, null, null)?.use {
                if (it.moveToFirst()) it.getString(0) else null
            } ?: "Yap recovery file"
        }
    }
    override fun choosePhoto(onResult: FileResult) {
        photo = onResult
        pickPhoto.launch(PickVisualMediaRequest(ActivityResultContracts.PickVisualMedia.ImageOnly))
    }
    override fun saveRecovery(ciphertext: String, onResult: FileResult) {
        save = ciphertext to onResult
        create.launch("yap-recovery.age")
    }
    override fun loadRecovery(onResult: FileResult) { load = onResult; open.launch(arrayOf("*/*")) }

    override fun saveKey(key: String, label: String, onResult: (String?) -> Unit) {
        activity.lifecycleScope.launch {
            try {
                credentials.createCredential(activity, CreatePasswordRequest("Yap · $label", key))
                onResult(null)
            } catch (e: CreateCredentialCancellationException) { onResult("Save canceled.") }
            catch (e: CreateCredentialException) { onResult("Couldn’t save the key. You can copy it to your password manager.") }
        }
    }
    override fun chooseKey(onResult: FileResult) {
        activity.lifecycleScope.launch {
            try {
                val credential = credentials.getCredential(activity, GetCredentialRequest(listOf(GetPasswordOption()))).credential as PasswordCredential
                onResult(credential.password, null)
            } catch (e: GetCredentialCancellationException) { onResult(null, "Canceled.") }
            catch (e: GetCredentialException) { onResult(null, "No saved key available. You can paste your recovery key.") }
        }
    }
    override fun hasKey(key: String) = false // Only the provider can enumerate credentials.
    override fun removeKey(key: String, onResult: (String?) -> Unit) = onResult("Manage this key in your password manager.")
}
