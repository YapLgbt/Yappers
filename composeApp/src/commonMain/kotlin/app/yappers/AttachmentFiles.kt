package app.yappers

import androidx.compose.foundation.layout.Column
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.*
import androidx.compose.ui.Modifier
import kotlinx.coroutines.CancellationException
import kotlinx.coroutines.launch
import kotlinx.serialization.Serializable

internal const val MAX_ATTACHMENT_BYTES = 20_000_000
internal const val ATTACHMENT_CHUNK_BYTES = 256 * 1024

@Serializable
data class Attachment(val id: String, val key: String, val name: String, val size: Int)

/** Picked file bytes cross the platform bridge one chunk at a time. */
class SelectedAttachment(val name: String, val size: Int, private val part: (Int) -> String) {
    internal fun readPart(index: Int) = part(index)
    internal var uploaded: Pair<String, Attachment>? = null
}

interface AttachmentFiles {
    fun chooseAttachment(onResult: (SelectedAttachment?, String?) -> Unit)
    fun saveAttachment(name: String, parts: List<String>, onResult: (String?) -> Unit)
}

internal val LocalAttachmentFiles = staticCompositionLocalOf<AttachmentFiles?> { null }

@Composable
internal fun AttachmentDownload(
    attachment: Attachment,
    download: suspend (Attachment) -> List<String>,
) {
    val files = LocalAttachmentFiles.current ?: return
    val control = LocalFeedControl.current
    var busy by remember(attachment.id) { mutableStateOf(false) }
    var error by remember(attachment.id) { mutableStateOf<String?>(null) }
    val scope = rememberCoroutineScope()
    val label = if (busy) "Downloading…" else "Download ${attachment.name}"
    fun start() {
        if (busy) return
        busy = true
        error = null
        scope.launch {
            try {
                val parts = download(attachment)
                files.saveAttachment(attachment.name, parts) {
                    error = it
                    busy = false
                }
            } catch (failure: Exception) {
                busy = false
                if (failure is CancellationException) throw failure
                error = "Couldn’t download this attachment. Try again."
            }
        }
    }
    Column {
        if (control != null) control(FormRow("download-${attachment.id}", label, enabled = !busy, onTap = ::start), Modifier)
        else TextButton(enabled = !busy, onClick = ::start) { Text(label) }
        error?.let { Text(it, color = MaterialTheme.colorScheme.error) }
    }
}
