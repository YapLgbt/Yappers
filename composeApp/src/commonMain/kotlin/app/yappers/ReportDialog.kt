package app.yappers

import androidx.compose.foundation.layout.*
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.unit.dp
import kotlinx.coroutines.CancellationException
import kotlinx.coroutines.launch
import kotlinx.serialization.json.*

internal data class ReportTarget(val subject: String = "account", val reference: String? = null, val text: String = "", val lookup: Boolean = false)
private val reasons = mapOf("harassment" to "Harassment", "outing" to "Outing", "stalking" to "Stalking", "threat" to "Threat",
    "intimateImage" to "Nonconsensual intimate image", "other" to "Other")

@Composable
internal fun ReportDialog(target: ReportTarget, controller: NetkitController, onClose: () -> Unit) {
    val scope = rememberCoroutineScope()
    var phase by remember { mutableStateOf(if (target.lookup) "lookup" else "edit") }
    var text by remember { mutableStateOf(if (target.subject == "message") "" else target.text.take(2000)) }
    var include by remember { mutableStateOf(false) }
    var category by remember { mutableStateOf("other") }
    var menu by remember { mutableStateOf(false) }
    var receipt by remember { mutableStateOf("") }
    var home by remember { mutableStateOf("") }
    var busy by remember { mutableStateOf(false) }
    var outcome by remember { mutableStateOf("") }
    var error by remember { mutableStateOf("") }
    fun advance() {
        if (busy) return
        busy = true
        error = ""
        scope.launch {
            try {
                if (phase == "edit") {
                    val draft = controller.reportDraft()
                    receipt = draft.getValue("receipt").jsonPrimitive.content
                    home = draft.getValue("home").jsonPrimitive.content
                    phase = "review"
                } else {
                    val result = if (phase == "lookup") controller.reportStatus(receipt.trim()) else
                        controller.submitReport(receipt, buildJsonObject {
                            put("subject", target.subject); put("category", category); put("text", text)
                            if (include) put("reference", target.reference)
                        })
                    outcome = when (result.getValue("status").jsonPrimitive.content) {
                        "received" -> "Received. Awaiting review."
                        "actionTaken" -> "Reviewed. Your home took action."
                        "noAction" -> "Reviewed. Your home closed the case without action."
                        "expired" -> "The evidence expired before the case was closed."
                        else -> "Unknown case status."
                    }
                    text = ""
                    phase = "done"
                }
            } catch (e: CancellationException) {
                throw e
            } catch (failure: Exception) {
                val reason = failure.message.orEmpty()
                error = when {
                    phase == "review" && "remove recovery keys" in reason -> "Edit the excerpt to remove recovery keys and private receipts."
                    phase == "review" && "draft expired" in reason -> "This draft expired. Choose Edit and review it again for a new receipt."
                    phase == "review" && "inbox is full" in reason -> "Your home’s inbox is full. Try again later."
                    phase == "lookup" -> "Could not retrieve the case. Check your home, connection and receipt. Receipts expire after 30 days."
                    phase == "review" -> "Delivery was not confirmed. Save this receipt to check the case or retry."
                    else -> "Your primary home’s report inbox is unavailable. No evidence was sent."
                }
            } finally { busy = false }
        }
    }
    LocalNativeForm.current?.let { form ->
        val title = if (phase == "lookup" || phase == "done") "Report status" else "Report to your home"
        val rows = buildList {
            when (phase) {
                "edit" -> {
                    add(FormRow("reason", "Reason", "menu", value = reasons.getValue(category), enabled = !busy,
                        choices = reasons.map { (id, label) -> FormChoice(label, id == category) { category = id } }))
                    add(FormRow("evidence", "Evidence for the moderator", "multiline", value = text, enabled = !busy,
                        limit = 2000, onChange = { if (it.length <= 2000) text = it }))
                    if (target.subject == "message" && text.isEmpty() && target.text.isNotEmpty())
                        add(FormRow("excerpt", "Use this message’s text", enabled = !busy) { text = target.text.take(2000) })
                    add(FormRow("privacy", "Edit or remove private details. Never include recovery keys. Selected message text will be readable by your moderator.", "text"))
                    target.reference?.let {
                        val label = if (target.subject == "message" || target.subject == "account") "Include account ID" else "Include ${target.subject} ID"
                        add(FormRow("include", label, "toggle", enabled = !busy, checked = include, onToggle = { selected -> include = selected }))
                        if (include) add(FormRow("reference", "Subject ID", "selectable", value = it))
                    }
                }
                "review" -> {
                    add(FormRow("home", "Receiving home", "text", detail = home))
                    add(FormRow("reason", "${target.subject.replaceFirstChar { it.uppercase() }} report", "text", detail = reasons.getValue(category)))
                    if (include) add(FormRow("reference", "Subject ID", "selectable", value = target.reference.orEmpty()))
                    if (text.isNotEmpty()) add(FormRow("excerpt", "Evidence for the moderator", "selectable", value = text))
                    add(FormRow("retention", "Deleting the original leaves this report in place. Yap’s software clears the evidence on closure or after seven days. Your operator can copy it. Save the receipt to check the outcome for 30 days. Anyone holding it can read the status.", "text"))
                }
                "done" -> add(FormRow("outcome", outcome, "text"))
            }
            if (phase != "edit") add(FormRow("receipt", "Private receipt", if (phase == "lookup") "field" else "selectable",
                value = receipt, keyboard = "ascii", enabled = !busy, onChange = { receipt = it }))
            if (error.isNotEmpty()) add(FormRow("error", error, "text"))
            // Keep long action titles in the form where they can wrap at larger text sizes.
            if (phase != "done") add(FormRow("submit", when (phase) { "edit" -> "Review report"; "lookup" -> "Check status"; else -> "Send these details" },
                enabled = !busy && when (phase) { "edit" -> text.isNotBlank() || include; "lookup" -> receipt.isNotBlank(); else -> true },
                onTap = ::advance))
        }
        form(NativeFormModel(title, listOf(FormSection(rows = rows)), leading = if (busy || phase == "done") null else
            FormChoice(if (phase == "review") "Edit" else "Cancel") {
                if (phase == "review") { phase = "edit"; error = "" } else onClose()
            }, trailing = if (phase == "done") FormChoice("Done", action = onClose) else null,
            canDismiss = !busy, onDismiss = { if (!busy) onClose() }), "sheet", Modifier)
        return
    }
    AlertDialog(onDismissRequest = { if (!busy) onClose() },
        title = { Text(if (phase == "lookup" || phase == "done") "Report status" else "Report to your home") },
        text = { Column(Modifier.verticalScroll(rememberScrollState()), verticalArrangement = Arrangement.spacedBy(8.dp)) {
            when (phase) {
                "edit" -> {
                    Box {
                        TextButton(onClick = { menu = true }, enabled = !busy) { Text("Reason: " + reasons.getValue(category)) }
                        DropdownMenu(expanded = menu, onDismissRequest = { menu = false }) {
                            reasons.forEach { (id, label) -> DropdownMenuItem(text = { Text(label) }, onClick = { category=id; menu=false }) }
                        }
                    }
                    OutlinedTextField(text, { if (it.length <= 2000) text = it }, enabled = !busy, keyboardOptions = KeyboardOptions(autoCorrectEnabled = false), label = { Text("Evidence for the moderator") })
                    if (target.subject == "message" && text.isEmpty() && target.text.isNotEmpty()) {
                        TextButton(onClick = { text = target.text.take(2000) }, enabled = !busy) { Text("Use this message’s text") }
                    }
                    Text("Edit or remove private details. Never include recovery keys. Selected message text will be readable by your moderator.")
                    target.reference?.let {
                        Row(verticalAlignment = Alignment.CenterVertically) {
                            Checkbox(include,{ value -> include = value }, enabled = !busy)
                            Text(if (target.subject == "message" || target.subject == "account") "Include account ID" else "Include ${target.subject} ID")
                        }
                        if (include) Text(it)
                    }
                }
                "review" -> {
                    Text("Receiving home: $home")
                    Text("${target.subject.replaceFirstChar { it.uppercase() }} report · ${reasons.getValue(category)}")
                    if (include) Text("Subject ID: ${target.reference}")
                    if (text.isNotEmpty()) Text(text)
                    Text("Deleting the original leaves this report in place. Yap’s software clears the evidence on closure or after seven days. Your operator can copy it. Save the receipt to check the outcome for 30 days. Anyone holding it can read the status.")
                }
                "done" -> Text(outcome)
            }
            if (phase != "edit") OutlinedTextField(receipt, { receipt = it }, readOnly = phase != "lookup" || busy, keyboardOptions = KeyboardOptions(autoCorrectEnabled = false),
                label = { Text("Private receipt") })
            if (error.isNotEmpty()) Text(error, color = MaterialTheme.colorScheme.error)
        } },
        confirmButton = {
            if (phase == "done") TextButton(onClick = onClose) { Text("Done") }
            else Button(onClick = ::advance, enabled = !busy && when (phase) {
                "edit" -> text.isNotBlank() || include
                "lookup" -> receipt.isNotBlank()
                else -> true
            }) { Text(when (phase) { "edit" -> "Review report"; "lookup" -> "Check status"; else -> "Send these details" }) }
        },
        dismissButton = {
            if (phase == "review") TextButton(onClick = { phase = "edit"; error = "" }, enabled = !busy) { Text("Edit") }
            else if (phase != "done") TextButton(onClick = onClose, enabled = !busy) { Text("Cancel") }
        })
}
