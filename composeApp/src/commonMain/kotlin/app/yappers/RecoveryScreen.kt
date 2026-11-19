@file:OptIn(androidx.compose.material3.ExperimentalMaterial3Api::class)
package app.yappers

import androidx.compose.foundation.layout.*
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.selection.toggleable
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.foundation.text.selection.SelectionContainer
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.input.KeyboardType
import androidx.compose.ui.text.input.PasswordVisualTransformation
import androidx.compose.ui.unit.dp
import kotlinx.coroutines.CancellationException
import kotlinx.coroutines.launch
import kotlinx.serialization.json.*

/** System pickers exchange age file bytes as hex with the owned JSON ABI. Null/null is cancellation. */
interface RecoveryKeyVault {
    fun saveKey(key: String, label: String, onResult: (String?) -> Unit)
    fun chooseKey(onResult: (String?, String?) -> Unit)
    fun hasKey(key: String): Boolean
    fun removeKey(key: String, onResult: (String?) -> Unit)
}

internal interface PasswordManagerVault : RecoveryKeyVault

interface RecoveryFiles {
    fun saveRecovery(ciphertext: String, onResult: (String?, String?) -> Unit)
    fun loadRecovery(onResult: (String?, String?) -> Unit)
}

@Composable
internal fun DataOwnershipSection(controller: NetkitController, connection: NetkitState, files: RecoveryFiles,
    alias: String, profile: CommunityProfile, onRestore: suspend (JsonObject) -> Unit, onError: (String) -> Unit) {
    val scope = rememberCoroutineScope()
    var mode by remember { mutableStateOf<String?>(null) }
    var passphrase by remember { mutableStateOf("") }
    var confirmation by remember { mutableStateOf("") }
    var invitation by remember { mutableStateOf("") }
    var ciphertext by remember { mutableStateOf<String?>(null) }
    var pendingAsDrafts by remember { mutableStateOf(false) }
    var working by remember { mutableStateOf(false) }
    var refresh by remember { mutableStateOf(false) }
    var notice by remember { mutableStateOf<String?>(null) }
    var recoveryKey by remember { mutableStateOf<String?>(null) }
    var showFiles by remember { mutableStateOf(false) }
    var historyOpen by remember { mutableStateOf(false) }
    fun close() { if (!working) { mode = null; passphrase = ""; confirmation = ""; invitation = ""; ciphertext = null; pendingAsDrafts = false; refresh = false } }
    val native = LocalNativeForm.current
    var formError by remember(mode) { mutableStateOf<String?>(null) }
    fun loadKey() {
        if (working) return
        working = true
        scope.launch {
            try { recoveryKey = controller.recoveryKey() }
            catch (failure: Exception) { if (failure is CancellationException) throw failure; onError(failure.message ?: "Connect to your home and try again.") }
            finally { working = false }
        }
    }
    if (native != null) {
        val sections = buildList {
            add(FormSection("Account recovery", rows = buildList {
                add(FormRow("get-key", "Get recovery key", enabled = connection.ownershipReady && !working, onTap = ::loadKey))
                add(FormRow("files", if (showFiles) "Hide backup files" else "Backup files") { showFiles = !showFiles })
                if (showFiles) {
                    add(FormRow("export", "Save a backup file", enabled = connection.ownershipReady && !working) { mode = "export"; notice = null })
                    add(FormRow("move", "Move to another community", enabled = !working) { mode = "import"; notice = null })
                    if (connection.ownedHistory.isNotEmpty()) add(FormRow("history", "Backed-up posts and replies") { historyOpen = true })
                }
                notice?.let { add(FormRow("notice", it, "text")) }
            }, footer = if (connection.ownershipReady) "Save a recovery key so you can use this account on a new device."
                        else "Connect to your community to get your recovery key."))
            if (connection.recoveredDrafts.isNotEmpty()) {
                add(FormSection("Recovered drafts", rows = connection.recoveredDrafts.mapIndexed { index, event ->
                    val action = event.getValue("payload").jsonObject.getValue("action").jsonObject
                    val kind = action.getValue("kind").jsonPrimitive.content
                    val title = action["headline"]?.jsonPrimitive?.content?.takeIf { it.isNotBlank() } ?: "Unsent $kind"
                    val body = action["body"]?.jsonPrimitive?.content ?: "Action: $kind · Target: ${action["target"]?.jsonPrimitive?.content.orEmpty()}"
                    FormRow("draft-$index", title, "selectable", value = body)
                }, footer = "Check the conversation before reposting a draft. It may already be there."))
            }
        }
        native(NativeFormModel(sections = sections), "inline", Modifier.fillMaxWidth())
    } else Column(verticalArrangement = Arrangement.spacedBy(12.dp)) {
        if (LocalMaterialNavigation.current == null) HorizontalDivider()
        if (LocalMaterialNavigation.current == null) Text("Account recovery", Modifier.padding(top = 8.dp), style = MaterialTheme.typography.titleLarge, fontWeight = FontWeight.Bold)
        Text(if (connection.ownershipReady) "Save a recovery key so you can use this account on a new device."
            else "Connect to your community to get your recovery key.", color = MaterialTheme.colorScheme.onSurfaceVariant)
        FlowRow(Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.spacedBy(8.dp)) {
            OutlinedButton(enabled = connection.ownershipReady && !working, onClick = ::loadKey) { Text("Get recovery key") }
            TextButton(onClick = { showFiles = !showFiles }) {
                Text(if (showFiles) "Hide backup files" else "Backup files")
            }
        }
        if (showFiles) FlowRow(Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.spacedBy(8.dp)) {
            OutlinedButton(enabled = connection.ownershipReady && !working, onClick = { mode = "export"; notice = null }) { Text("Save a backup file") }
            OutlinedButton(enabled = !working, onClick = { mode = "import"; notice = null }) { Text("Move to another community") }
            if (connection.ownedHistory.isNotEmpty()) TextButton(onClick = { historyOpen = true }) { Text("Backed-up posts and replies") }
        }
        notice?.let { Text(it, color = MaterialTheme.colorScheme.onSurfaceVariant, style = MaterialTheme.typography.bodyMedium) }
        if (connection.recoveredDrafts.isNotEmpty()) {
            Text("Recovered drafts", style = MaterialTheme.typography.titleMedium, fontWeight = FontWeight.Bold)
            Text("Check the conversation before reposting a draft. It may already be there.", style = MaterialTheme.typography.bodyMedium)
            connection.recoveredDrafts.forEach { event ->
                val action = event.getValue("payload").jsonObject.getValue("action").jsonObject
                SelectionContainer {
                    Column(Modifier.fillMaxWidth().padding(vertical = 8.dp), verticalArrangement = Arrangement.spacedBy(6.dp)) {
                        val kind = action.getValue("kind").jsonPrimitive.content
                        Text(action["headline"]?.jsonPrimitive?.content?.takeIf { it.isNotBlank() } ?: "Unsent $kind", fontWeight = FontWeight.SemiBold)
                        Text(action["body"]?.jsonPrimitive?.content ?: "Action: $kind · Target: ${action["target"]?.jsonPrimitive?.content.orEmpty()}")
                        HorizontalDivider()
                    }
                }
            }
        }
    }
    recoveryKey?.let { key ->
        RecoveryKeyDialog(key, alias, files as? RecoveryKeyVault) { recoveryKey = null }
    }
    if (historyOpen) {
        val deleted = connection.ownedHistory.mapNotNull { envelope ->
            val action = envelope.getValue("event").jsonObject.getValue("payload").jsonObject.getValue("action").jsonObject
            if (action["kind"]?.jsonPrimitive?.content == "delete") action["target"]?.jsonPrimitive?.content else null
        }.toSet()
        val entries = connection.ownedHistory.sortedByDescending { it["acceptedAt"]?.jsonPrimitive?.longOrNull ?: 0 }
            .filter { envelope ->
                val event = envelope.getValue("event").jsonObject
                event["id"]?.jsonPrimitive?.content !in deleted && event.getValue("payload").jsonObject.getValue("action").jsonObject["kind"]?.jsonPrimitive?.content in listOf("post", "postV2", "reply")
            }
        if (native != null) {
            val rows = entries.map { envelope ->
                val event = envelope.getValue("event").jsonObject
                val action = event.getValue("payload").jsonObject.getValue("action").jsonObject
                val title = action["headline"]?.jsonPrimitive?.content?.takeIf { it.isNotBlank() }
                    ?: if (action["kind"]?.jsonPrimitive?.content == "reply") "Your comment" else "Your post"
                FormRow(event.getValue("id").jsonPrimitive.content, title, "selectable", value = action.getValue("body").jsonPrimitive.content)
            }.ifEmpty { listOf(FormRow("empty", "No posts or replies in this backup.", "text")) }
            native(NativeFormModel("Backed-up posts and replies", sections = listOf(FormSection(rows = rows,
                footer = "Only you can see this copy.")), trailing = FormChoice("Done") { historyOpen = false },
                onDismiss = { historyOpen = false }), "sheet", Modifier)
        } else EditorSheet(onDismissRequest = { historyOpen = false }, sheetState = rememberModalBottomSheetState(skipPartiallyExpanded = true)) {
            Row(Modifier.fillMaxWidth().padding(horizontal = 22.dp), horizontalArrangement = Arrangement.End) {
                TextButton(onClick = { historyOpen = false }) { Text("Done") }
            }
            LazyColumn(Modifier.weight(1f), contentPadding = PaddingValues(22.dp), verticalArrangement = Arrangement.spacedBy(16.dp)) {
                item { Text("Backed-up posts and replies", style = MaterialTheme.typography.headlineMedium, fontWeight = FontWeight.Bold) }
                item { Text("Only you can see this copy.", color = MaterialTheme.colorScheme.onSurfaceVariant) }
                if (entries.isEmpty()) item { Text("No posts or replies in this backup.") }
                items(entries.size) { index ->
                    val action = entries[index].getValue("event").jsonObject.getValue("payload").jsonObject.getValue("action").jsonObject
                    SelectionContainer {
                        Column(verticalArrangement = Arrangement.spacedBy(8.dp)) {
                            Text(action["headline"]?.jsonPrimitive?.content?.takeIf { it.isNotBlank() } ?: if (action["kind"]?.jsonPrimitive?.content == "reply") "Your comment" else "Your post", style = postTitleStyle)
                            Text(action.getValue("body").jsonPrimitive.content, style = postBodyStyle)
                            HorizontalDivider()
                        }
                    }
                }
            }
        }
    }
    mode?.let { currentMode ->
        val exporting = currentMode == "export"
        val canSubmit = !working && if (exporting) passphrase.length >= 12 && passphrase == confirmation
            else passphrase.isNotEmpty() && !ciphertext.isNullOrBlank() && invitation.isNotBlank()
        fun report(message: String) {
            if (native != null) formError = message else onError(message)
        }
        fun chooseBackup() {
            files.loadRecovery { value, error ->
                if (error != null) report(error) else if (value != null) { ciphertext = value; formError = null }
            }
        }
        fun submit() {
            if (!canSubmit || working) return
            formError = null
            working = true
            scope.launch {
                try {
                    if (exporting) {
                        val encrypted = controller.exportRecovery(passphrase, buildJsonObject {
                            put("alias", alias); put("profile", Json.encodeToJsonElement(CommunityProfile.serializer(), profile))
                        }, refreshFromHome = refresh)
                        files.saveRecovery(encrypted) { saved, error ->
                            working = false
                            if (error != null) report(error)
                            else if (saved != null) { close(); notice = "Backup file saved." }
                        }
                    } else {
                        val local = controller.importRecovery(ciphertext!!, passphrase, invitation, alias, pendingAsDrafts)
                        onRestore(local)
                        working = false; close(); notice = "Account moved. You can now rejoin your groups."
                    }
                } catch (failure: Exception) {
                    working = false
                    if (failure is CancellationException) throw failure
                    report(failure.message ?: "Could not finish recovery. Keep your original backup and try again.")
                }
            }
        }
        if (native != null) {
            val rows = buildList {
                if (!exporting) {
                    add(FormRow("choose-file", if (ciphertext == null) "Choose backup file" else "Change backup file", enabled = !working, onTap = ::chooseBackup))
                    if (ciphertext != null) add(FormRow("file-selected", "Encrypted backup selected", "text"))
                }
                add(FormRow("password", "Backup password", "secure", value = passphrase, enabled = !working, limit = 1024,
                    onChange = { if (it.length <= 1024) passphrase = it }))
                if (exporting) {
                    add(FormRow("confirm-password", "Confirm password", "secure", value = confirmation, enabled = !working, limit = 1024,
                        onChange = { confirmation = it.take(1024) }))
                    add(FormRow("older", "Include older posts and replies", "toggle", detail = "Requires an internet connection.",
                        checked = refresh, enabled = !working, onToggle = { refresh = it }))
                } else {
                    add(FormRow("new-invitation", "New community invitation", "secure", value = invitation, enabled = !working, limit = 8192,
                        onChange = { invitation = it.take(8192) }))
                    add(FormRow("pending", "Keep unfinished posts as drafts", "toggle", detail = "You can review these drafts before posting.",
                        checked = pendingAsDrafts, enabled = !working, onToggle = { pendingAsDrafts = it }))
                }
                formError?.let { add(FormRow("problem", it, "text")) }
            }
            val intro = if (exporting) "This file holds your account, profile, and saved history. Keep the password somewhere safe. Yap can’t recover it."
                else "You’ll need your backup file, its password, and an invitation from the new community. Moving changes which community manages your account."
            native(NativeFormModel(if (exporting) "Backup file" else "Move home",
                sections = listOf(FormSection(rows = listOf(FormRow("intro", intro, "text"))),
                    FormSection(rows = rows, footer = if (exporting) "Use at least 12 characters." else "Keep the original file until the move is complete.")),
                leading = FormChoice("Cancel", action = ::close), leadingEnabled = !working,
                trailing = FormChoice(if (working) "Working…" else if (exporting) "Save backup" else "Move account", action = ::submit),
                trailingEnabled = canSubmit, canDismiss = !working, onDismiss = ::close), "sheet", Modifier)
            return@let
        }
        val sheet = rememberModalBottomSheetState(skipPartiallyExpanded = true, confirmValueChange = { !working })
        EditorSheet(onDismissRequest = { close() }, sheetState = sheet) {
            Column(Modifier.fillMaxSize().imePadding().padding(top = 12.dp)) {
                Row(Modifier.fillMaxWidth().padding(horizontal = 22.dp), verticalAlignment = Alignment.CenterVertically) {
                    YapCancelButton(enabled = !working, onClick = { close() })
                    Spacer(Modifier.weight(1f))
                    YapButton(enabled = canSubmit, onClick = ::submit) { Text(if (working) "Working…" else if (exporting) "Save backup" else "Move account") }
                }
                LazyColumn(Modifier.weight(1f), contentPadding = PaddingValues(22.dp), verticalArrangement = Arrangement.spacedBy(16.dp)) {
                    item { Text(if (exporting) "Save a backup file" else "Move to another community", style = MaterialTheme.typography.headlineMedium, fontWeight = FontWeight.Bold) }
                    item { Text(if (exporting) "This file holds your account, profile, and saved history. Keep the password somewhere safe. Yap can’t recover it." else "You’ll need your backup file, its password, and an invitation from the new community. Moving changes which community manages your account.", color = MaterialTheme.colorScheme.onSurfaceVariant) }
                    if (!exporting) item {
                        OutlinedButton(enabled = !working, onClick = ::chooseBackup) { Text(if (ciphertext == null) "Choose backup file" else "Change backup file") }
                        if (ciphertext != null) Text("Encrypted backup selected", style = MaterialTheme.typography.bodySmall)
                    }
                    item { OutlinedTextField(value = passphrase, enabled = !working, onValueChange = { if (it.length <= 1024) passphrase = it }, label = { Text(if (exporting) "Backup password" else "Backup password") },
                        visualTransformation = PasswordVisualTransformation(), keyboardOptions = KeyboardOptions(keyboardType = KeyboardType.Password), modifier = Modifier.fillMaxWidth(), singleLine = true,
                        supportingText = { Text(if (exporting) "Use at least 12 characters." else "Enter the password you chose when you saved the file.") }) }
                    if (exporting) {
                        item { OutlinedTextField(value = confirmation, enabled = !working, onValueChange = { confirmation = it.take(1024) }, label = { Text("Confirm password") }, visualTransformation = PasswordVisualTransformation(), keyboardOptions = KeyboardOptions(keyboardType = KeyboardType.Password), modifier = Modifier.fillMaxWidth(), singleLine = true) }
                        item { Row(Modifier.fillMaxWidth().toggleable(value = refresh, enabled = !working, role = Role.Checkbox,
                            onValueChange = { refresh = it }).heightIn(min = 48.dp), verticalAlignment = Alignment.CenterVertically) {
                            Checkbox(checked = refresh, enabled = !working, onCheckedChange = null)
                            Spacer(Modifier.width(12.dp))
                            Text("Include older posts and replies", Modifier.weight(1f))
                        }; Text("Requires an internet connection.", style = MaterialTheme.typography.bodySmall, color = MaterialTheme.colorScheme.onSurfaceVariant) }
                    } else {
                        item { OutlinedTextField(value = invitation, enabled = !working, onValueChange = { invitation = it.take(8192) }, label = { Text("New community invitation") }, visualTransformation = PasswordVisualTransformation(), modifier = Modifier.fillMaxWidth(), minLines = 4, maxLines = 8) }
                        item { Row(Modifier.fillMaxWidth().toggleable(value = pendingAsDrafts, enabled = !working, role = Role.Checkbox,
                            onValueChange = { pendingAsDrafts = it }).heightIn(min = 48.dp), verticalAlignment = Alignment.CenterVertically) {
                            Checkbox(checked = pendingAsDrafts, enabled = !working, onCheckedChange = null)
                            Spacer(Modifier.width(12.dp))
                            Text("Keep unfinished posts as drafts", Modifier.weight(1f))
                        }; Text("You can review these drafts before posting.", style = MaterialTheme.typography.bodySmall, color = MaterialTheme.colorScheme.onSurfaceVariant) }
                        item { Text("Keep the original file until the move is complete.", style = MaterialTheme.typography.bodyMedium) }
                    }
                }
            }
        }
    }
}
