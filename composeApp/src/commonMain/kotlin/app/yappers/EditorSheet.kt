@file:OptIn(androidx.compose.material3.ExperimentalMaterial3Api::class)

package app.yappers

import androidx.compose.foundation.layout.ColumnScope
import androidx.compose.foundation.layout.fillMaxHeight
import androidx.compose.foundation.layout.statusBarsPadding
import androidx.compose.material3.ModalBottomSheet
import androidx.compose.material3.SheetValue
import androidx.compose.material3.rememberModalBottomSheetState
import androidx.compose.runtime.remember
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.setValue
import androidx.compose.runtime.rememberUpdatedState
import androidx.compose.material3.SheetState
import androidx.compose.runtime.Composable
import androidx.compose.ui.Modifier

// Material keys SheetState on the guard's identity. A first edit changes whether dismissal
// is allowed, but must never replace the sheet or interrupt the focused field.
@Composable
internal fun rememberEditorSheetState(confirmValueChange: (SheetValue) -> Boolean): SheetState {
    val latest = rememberUpdatedState(confirmValueChange)
    val guard = remember { { value: SheetValue -> latest.value(value) } }
    return rememberModalBottomSheetState(skipPartiallyExpanded = true, confirmValueChange = guard)
}

/**
 * Long forms own their vertical scroll gestures. A draggable parent sheet would consume
 * the list's boundary overscroll, moving the action row and bouncing the whole editor.
 * Set height on the sheet itself, rather than sizing it from its own inset-adjusted child.
 * Cancel/save and guarded outside dismissal remain available; no drag handle promises
 * a swipe-to-dismiss interaction that competes with scrolling the form.
 */
@Composable
internal fun EditorSheet(
    onDismissRequest: () -> Unit,
    sheetState: SheetState,
    content: @Composable ColumnScope.() -> Unit
) {
    ModalBottomSheet(
        onDismissRequest = onDismissRequest,
        sheetState = sheetState,
        modifier = Modifier.fillMaxHeight().statusBarsPadding(),
        sheetGesturesEnabled = false,
        dragHandle = null,
        content = content
    )
}

class PostDraft internal constructor() {
    val headlineLimit = HEADLINE_LIMIT
    val bodyLimit = POST_LIMIT
    val topicChoices = topics.drop(1)
    var headline by mutableStateOf("")
        private set
    var body by mutableStateOf("")
        private set
    var category by mutableStateOf("Chatter")
        private set
    val dirty: Boolean get() = headline.isNotBlank() || body.isNotBlank()
    val complete: Boolean get() = headline.isNotBlank() && body.isNotBlank()

    fun editHeadline(text: String) { if (text.length <= headlineLimit) headline = text }
    fun editBody(text: String) { if (text.length <= bodyLimit) body = text }
    fun selectTopic(topic: String) { if (topic in topicChoices) category = topic }
}

class PostForm internal constructor(
    val draft: PostDraft,
    val destination: String,
    val namedIdentity: String,
    val anonymous: Boolean,
    val networked: Boolean,
    val usernameClaim: EditorUsernameClaim?,
    val submitting: Boolean,
    val canPublish: Boolean,
    val publishLabel: String,
    val error: String?,
    private val onIdentity: (Boolean) -> Unit,
    private val onDismissError: () -> Unit,
    internal val dismiss: () -> Unit,
    internal val publish: () -> Unit,
) {
    fun selectIdentity(anonymous: Boolean) { if (!submitting) onIdentity(anonymous) }
    fun dismissError() = onDismissError()
}

internal typealias PostSheetPresenter = @Composable (PostForm) -> Unit

internal val LocalPostSheetPresenter = androidx.compose.runtime.staticCompositionLocalOf<PostSheetPresenter?> { null }
