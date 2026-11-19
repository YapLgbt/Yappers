package app.yappers

import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.selection.selectable
import androidx.compose.foundation.selection.selectableGroup
import androidx.compose.foundation.selection.toggleable
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.ui.Alignment
import androidx.compose.ui.draw.clip
import androidx.compose.ui.Modifier
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.unit.dp

private fun ageLabel(days: Int) = when (days) {
    0 -> "Any age"
    1 -> "1 day"
    else -> "$days days"
}

@Composable
internal fun PrivacyScreen(policy: DevicePrivacy, status: String?, onClose: () -> Unit,
                           onSave: ((PrivacySettings) -> PrivacySettings) -> Unit, onUnmute: (String) -> Unit, onReport: ((ReportTarget) -> Unit)? = null, nativeTitle: Boolean = false) {
    val settings = policy.settings
    var agePicker by remember { mutableStateOf<String?>(null) }
    LocalNativeForm.current?.let { form ->
        fun ageMenu(id: String, title: String, detail: String, current: Int, change: (Int) -> Unit) =
            FormRow(id, title, "menu", detail, ageLabel(current),
                choices = listOf(0, 1, 7, 30).map { days -> FormChoice(ageLabel(days), days == current) { change(days) } })
        val sections = buildList {
            if (status != null) add(FormSection(rows = listOf(FormRow("status", status, "text"))))
            if (onReport != null) add(FormSection("Reports", listOf(
                FormRow("report", "Report to your home", "disclosure") { onReport(ReportTarget()) },
                FormRow("receipt", "Check a private receipt", "disclosure") { onReport(ReportTarget(lookup = true)) })))
            add(FormSection("Feed and replies", listOf(
                FormRow("anonymous", "Hide anonymous content", "toggle", "Hide other people’s anonymous posts and replies, including saved posts.",
                    checked = settings.hideAnonymous, onToggle = { hidden -> onSave { it.copy(hideAnonymous = hidden) } }),
                ageMenu("feed-age", "Minimum account age", "Based on when your home first saw the account.", settings.feedDays) { days -> onSave { it.copy(feedDays = days) } })))
            add(FormSection("Direct messages", buildList {
                add(FormRow("approved", "Approved people only", "toggle", "Messages from anyone else go to Requests. Sending a message to someone also approves them.",
                    checked = settings.approvedOnly, onToggle = { approved -> onSave { it.copy(approvedOnly = approved) } }))
                if (!settings.approvedOnly) add(ageMenu("dm-age", "Inbox minimum age", "Route messages from newer accounts to Requests.", settings.dmDays) { days -> onSave { it.copy(dmDays = days) } })
            }))
            if (policy.mutedThreads.isNotEmpty()) add(FormSection("Muted threads", policy.mutedThreads.map { (id, title) ->
                FormRow(id, "Unmute", detail = title) { onUnmute(id) }
            }))
        }
        form(NativeFormModel("Privacy", sections), "screen", Modifier.fillMaxSize())
        return
    }
    val navigation = LocalMaterialNavigation.current
    Column(Modifier.fillMaxSize()) {
        if (navigation != null) navigation.UpBar("Privacy", onClose)
        else if (!nativeTitle) Row(Modifier.fillMaxWidth().padding(start = 22.dp, end = 22.dp, top = 12.dp), verticalAlignment = Alignment.CenterVertically) {
            Text("Privacy", Modifier.weight(1f), style = MaterialTheme.typography.headlineLarge)
            TextButton(onClick = onClose) { Text("Done") }
        }
        LazyColumn(Modifier.fillMaxWidth().weight(1f), contentPadding = PaddingValues(start = 22.dp, end = 22.dp, bottom = 12.dp)) {
            if (status != null) item {
                Text(status, style = MaterialTheme.typography.bodySmall, color = MaterialTheme.colorScheme.onSurfaceVariant)
            }
            if (onReport != null) item {
                PrivacyHeading("Reports")
                TextButton(onClick = { onReport(ReportTarget()) }) { Text("Report to your home") }
                TextButton(onClick = { onReport(ReportTarget(lookup = true)) }) { Text("Check a private receipt") }
            }
            item { PrivacyHeading("Feed and replies") }
            item { PrivacySwitch("Hide anonymous content", "Hide other people’s anonymous posts and replies, including saved posts.",
                settings.hideAnonymous) { hidden -> onSave { it.copy(hideAnonymous = hidden) } } }
            item { AgeRow("Minimum account age", settings.feedDays, "Based on when your home first saw the account.") { agePicker = "feed" } }
            item { PrivacyHeading("Direct messages") }
            item { PrivacySwitch("Approved people only", "Messages from anyone else go to Requests. Sending a message to someone also approves them.",
                settings.approvedOnly) { required -> onSave { it.copy(approvedOnly = required) } } }
            if (!settings.approvedOnly) item {
                AgeRow("Inbox minimum age", settings.dmDays, "Route messages from newer accounts to Requests.") { agePicker = "inbox" }
            }
            if (policy.mutedThreads.isNotEmpty()) {
                item { PrivacyHeading("Muted threads") }
                items(policy.mutedThreads.toList(), key = { it.first }) { (id, title) ->
                    Row(verticalAlignment = Alignment.CenterVertically) {
                        Text(title, Modifier.weight(1f), maxLines = 2)
                        TextButton(onClick = { onUnmute(id) }) { Text("Unmute") }
                    }
                }
            }
        }
    }
    agePicker?.let { target ->
        val feed = target == "feed"
        AlertDialog(onDismissRequest = { agePicker = null },
            title = { Text(if (feed) "Minimum account age" else "Inbox minimum age") },
            text = { Column(Modifier.selectableGroup()) {
                for (days in listOf(0, 1, 7, 30)) {
                    val selected = days == if (feed) settings.feedDays else settings.dmDays
                    Row(Modifier.fillMaxWidth().heightIn(min = 48.dp).selectable(selected, role = Role.RadioButton, onClick = {
                        agePicker = null
                        onSave { if (feed) it.copy(feedDays = days) else it.copy(dmDays = days) }
                    }), verticalAlignment = Alignment.CenterVertically) {
                        RadioButton(selected, onClick = null)
                        Text(ageLabel(days), Modifier.padding(start = 12.dp))
                    }
                }
            } }, confirmButton = { TextButton(onClick = { agePicker = null }) { Text("Cancel") } })
    }
}

@Composable
private fun PrivacyHeading(title: String) {
    val android = LocalMaterialNavigation.current != null
    Text(title, Modifier.padding(top = 24.dp, bottom = if (android) 8.dp else 4.dp),
        style = if (android) MaterialTheme.typography.titleLarge else MaterialTheme.typography.titleMedium,
        color = MaterialTheme.colorScheme.primary)
    if (!android) HorizontalDivider()
}

@Composable
private fun PrivacySwitch(label: String, detail: String, selected: Boolean, onChange: (Boolean) -> Unit) {
    if (LocalMaterialNavigation.current != null) {
        PrivacyListItem(label, detail, Modifier.toggleable(selected, role = Role.Switch, onValueChange = onChange)) {
            Switch(selected, onCheckedChange = null)
        }
        return
    }
    Row(Modifier.fillMaxWidth().toggleable(selected, role = Role.Switch, onValueChange = onChange)
        .padding(vertical = 14.dp), verticalAlignment = Alignment.CenterVertically) {
        Column(Modifier.weight(1f).padding(end = 12.dp)) {
            Text(label, style = MaterialTheme.typography.titleMedium)
            Text(detail, style = MaterialTheme.typography.bodyMedium, color = MaterialTheme.colorScheme.onSurfaceVariant)
        }
        Switch(selected, onCheckedChange = null)
    }
}

@Composable
private fun AgeRow(label: String, days: Int, detail: String, onClick: () -> Unit) {
    if (LocalMaterialNavigation.current != null) {
        PrivacyListItem(label, detail, Modifier.clickable(role = Role.Button, onClick = onClick)) {
            Text(ageLabel(days), style = MaterialTheme.typography.labelLarge, color = MaterialTheme.colorScheme.primary)
        }
        return
    }
    Column(Modifier.fillMaxWidth().clickable(role = Role.Button, onClick = onClick).padding(vertical = 14.dp)) {
        Row(verticalAlignment = Alignment.CenterVertically) {
            Text(label, Modifier.weight(1f), style = MaterialTheme.typography.titleMedium)
            Text(ageLabel(days), color = MaterialTheme.colorScheme.primary)
        }
        Text(detail, style = MaterialTheme.typography.bodyMedium, color = MaterialTheme.colorScheme.onSurfaceVariant)
    }
}

@Composable
private fun PrivacyListItem(label: String, detail: String, action: Modifier, trailing: @Composable () -> Unit) {
    ListItem(
        headlineContent = { Text(label) },
        supportingContent = { Text(detail) },
        trailingContent = trailing,
        colors = ListItemDefaults.colors(containerColor = MaterialTheme.colorScheme.surfaceContainer),
        modifier = Modifier.fillMaxWidth().padding(bottom = 4.dp).clip(MaterialTheme.shapes.medium).then(action)
    )
}
