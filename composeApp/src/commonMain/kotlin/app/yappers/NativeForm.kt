package app.yappers

import androidx.compose.runtime.Composable
import androidx.compose.runtime.staticCompositionLocalOf
import androidx.compose.ui.Modifier

/** Presentation values for UIKit. Feature composables own validation, drafts, and actions. */
class FormChoice(val title: String, val selected: Boolean = false, val destructive: Boolean = false, private val action: () -> Unit) {
    fun select() = action()
}

class FormRow(
    val id: String, val title: String, val kind: String = "action", val detail: String = "",
    value: String = "", val enabled: Boolean = true, val checked: Boolean = false,
    val choices: List<FormChoice> = emptyList(), val placeholder: String = "", val limit: Int = 0,
    val symbol: String = "", val destructive: Boolean = false, val identifier: String = "",
    val keyboard: String = "", private val fieldValue: (() -> String)? = null,
    private val onChange: (String) -> Unit = {}, private val onToggle: (Boolean) -> Unit = {},
    private val onTap: () -> Unit = {},
) {
    val value = fieldValue?.invoke() ?: value
    // UIKit may receive a form snapshot after the user has typed the next character.
    fun currentText(): String = fieldValue?.invoke() ?: value
    fun edit(text: String) = onChange(text)
    fun toggle(selected: Boolean) = onToggle(selected)
    fun tap() = onTap()
}

class FormSection(val title: String = "", val rows: List<FormRow>, val footer: String = "")

class FormConfirmation(val title: String, val message: String, val confirm: FormChoice, val cancel: FormChoice)

class NativeFormModel(
    val title: String = "", val sections: List<FormSection>,
    val leading: FormChoice? = null, val trailing: FormChoice? = null, val trailingEnabled: Boolean = true, val leadingEnabled: Boolean = true,
    val canDismiss: Boolean = true, val confirmation: FormConfirmation? = null,
    private val onRefresh: (() -> Unit)? = null, private val onDismiss: () -> Unit = {},
) {
    val refreshable: Boolean get() = onRefresh != null
    fun refresh() { onRefresh?.invoke() }
    fun dismiss() = onDismiss()
}

// mode: screen (fills supplied modifier), inline (intrinsic height), sheet (native modal).
internal val LocalNativeForm = staticCompositionLocalOf<(@Composable (NativeFormModel, String, Modifier) -> Unit)?> { null }

internal val LocalNativeConfirmation = staticCompositionLocalOf<(@Composable (FormConfirmation) -> Unit)?> { null }

@Composable
internal fun PlatformConfirmation(title: String, detail: String, action: String, onCancel: () -> Unit,
                                  cancel: String = "Cancel", destructive: Boolean = true, onConfirm: () -> Unit) {
    val native = LocalNativeConfirmation.current
    if (native != null) {
        native(FormConfirmation(title, detail, FormChoice(action, destructive = destructive, action = onConfirm),
            FormChoice(cancel, action = onCancel)))
    } else {
        androidx.compose.material3.AlertDialog(onDismissRequest = onCancel,
            title = { androidx.compose.material3.Text(title) }, text = { androidx.compose.material3.Text(detail) },
            confirmButton = { androidx.compose.material3.TextButton(onClick = onConfirm) { androidx.compose.material3.Text(action) } },
            dismissButton = if (cancel.isEmpty()) null else ({ YapCancelButton(onClick = onCancel, label = cancel) }))
    }
}
