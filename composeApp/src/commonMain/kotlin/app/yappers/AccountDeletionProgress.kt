package app.yappers

import androidx.compose.foundation.layout.*
import androidx.compose.material3.*
import androidx.compose.runtime.Composable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.unit.dp

@Composable
internal fun AccountDeletionProgress(status: DeletionStatus, submitting: Boolean, failure: String?, retry: () -> Unit) {
    val locked = status == DeletionStatus.StorageLocked
    val working = status == DeletionStatus.Working || submitting
    val retryLabel = if (locked) "Retry" else "Retry deletion"
    val detail = if (locked) "Unlock your device to open Yap." else if (working) "Deleting your account…" else failure ?: "Resume deletion to finish removing this account."
    val native = LocalNativeForm.current
    if (native != null) {
        val rows = buildList {
            add(FormRow("deletion-status", detail, kind = "text"))
            if (!working) add(FormRow("retry-deletion", retryLabel, onTap = retry))
        }
        native(NativeFormModel(if (locked) "Open Yap" else "Account deletion", listOf(FormSection(rows = rows))), "screen", Modifier.fillMaxSize())
    } else Surface(Modifier.fillMaxSize()) {
        Column(Modifier.fillMaxSize().padding(24.dp), verticalArrangement = Arrangement.Center,
            horizontalAlignment = Alignment.CenterHorizontally) {
            Text(detail)
            Spacer(Modifier.height(16.dp))
            if (working) CircularProgressIndicator() else Button(onClick = retry) { Text(retryLabel) }
        }
    }
}
