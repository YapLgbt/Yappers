package app.yappers

import androidx.compose.runtime.Composable
import androidx.compose.runtime.staticCompositionLocalOf

/** Current screen state and actions for the platform's inline writing controls. */
class InlineEditorModel internal constructor(
    private val currentBody: () -> String,
    val limit: Int,
    private val isSending: () -> Boolean,
    private val sendEnabled: () -> Boolean,
    val dismissKeyboardRequest: Int = 0,
    val namedIdentity: String? = null,
    val anonymous: Boolean = false,
    val networked: Boolean = false,
    val usernameClaim: EditorUsernameClaim? = null,
    private val onEdit: (String) -> Unit,
    private val onSend: () -> Unit,
    private val onChooseIdentity: (Boolean) -> Unit = {},
) {
    // The keyboard can edit and submit between two Compose renders.
    val body: String get() = currentBody()
    val sending: Boolean get() = isSending()
    val canSend: Boolean get() = sendEnabled()

    fun edit(body: String): Boolean {
        if (sending || body.length > limit) return false
        onEdit(body)
        return true
    }
    fun send() { if (canSend) onSend() }
    fun chooseIdentity(anonymous: Boolean) { if (!sending) onChooseIdentity(anonymous) }
}

internal val LocalInlineEditor = staticCompositionLocalOf<(@Composable (InlineEditorModel) -> Unit)?> { null }
