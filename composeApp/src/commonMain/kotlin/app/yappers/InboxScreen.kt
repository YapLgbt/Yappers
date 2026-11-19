package app.yappers

import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.lazy.rememberLazyListState
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.dp
import kotlinx.coroutines.launch

enum class InboxPage { Messages, Requests, Blocked, Privacy, Conversation }

@Composable
internal fun InboxScreen(
    connection: NetkitState, contact: MessageContact?, blocks: List<BlockedAccount>,
    onPrivacy: () -> Unit, onApprove: (String) -> Unit,
    onSelect: (MessageContact?) -> Unit, onClose: () -> Unit, onBrowse: () -> Unit, onJoin: () -> Unit, onRefresh: () -> Unit,
    onReport: ((ReportTarget) -> Unit)?, onBlock: (MessageContact) -> Unit, onUnblock: (String) -> Unit,
    nativePage: InboxPage? = null, openPage: (InboxPage) -> Unit = {},
    onSend: suspend (MessageContact, String) -> Boolean
) {
    val navigation = LocalMaterialNavigation.current
    var managing by remember { mutableStateOf(nativePage == InboxPage.Blocked) }
    var requests by remember { mutableStateOf(nativePage == InboxPage.Requests) }
    LaunchedEffect(nativePage) { if (nativePage == InboxPage.Blocked) onRefresh() }
    AppBackHandler(nativePage == null && contact == null && (managing || requests)) { managing = false; requests = false }
    var options by remember { mutableStateOf(false) }
    if (contact == null) {
        val recent = connection.messages.asReversed().distinctBy { it.peer }
        val emptyTitle = if (connection.mailboxElsewhere) "Private messages" else "No conversations yet"
        val emptyDetail = when {
            connection.mailboxElsewhere -> OTHER_MESSAGING_DEVICE
            !connection.enabled -> "Join a community to message other members."
            connection.online && !connection.directAvailable -> "Messaging is unavailable on this device."
            else -> "Tap someone’s name on a post, then choose Message."
        }
        val inboxStatus = connection.messageStatus.takeUnless { connection.mailboxElsewhere && recent.isEmpty() }
        LocalNativeForm.current?.let { form ->
            val sections = buildList {
                when {
                    managing -> {
                        if (blocks.isEmpty()) add(FormSection(rows = listOf(FormRow("empty", "You haven’t blocked anyone.", "text"))))
                        blocks.forEach { block -> add(FormSection(rows = listOf(FormRow("name", block.label, "text"),
                            FormRow("unblock", "Unblock") { onUnblock(block.id) }))) }
                    }
                    requests -> {
                        add(FormSection(rows = listOf(FormRow("info", if (connection.requests.isEmpty()) "No message requests." else "Approve a sender to read their messages.", "text"))))
                        connection.requests.forEach { peer ->
                            val label = connection.snapshot.posts.firstOrNull { it.visibility?.peer == peer }?.author ?: "Account ${peer.take(12)}"
                            add(FormSection(label, buildList {
                                add(FormRow("accept", "Accept") { onApprove(peer) })
                                if (onReport != null) add(FormRow("report", "Report account") { onReport(ReportTarget("account", peer)) })
                                add(FormRow("block", "Block", destructive = true) { onBlock(MessageContact(peer, label)) })
                            }))
                        }
                    }
                    else -> {
                        if (connection.requests.isNotEmpty()) add(FormSection(rows = listOf(
                            FormRow("requests", "Message requests (${connection.requests.size})", "disclosure") { openPage(InboxPage.Requests) })))
                        inboxStatus?.let { add(FormSection(rows = listOf(FormRow("status", it, "text")))) }
                        if (recent.isEmpty()) {
                            add(FormSection(rows = listOf(FormRow("empty", emptyTitle, "text", emptyDetail),
                                FormRow("browse", if (connection.enabled) "Browse posts" else "Community settings", onTap = if (connection.enabled) onBrowse else onJoin))))
                        } else add(FormSection(rows = recent.map { message -> FormRow(message.peer, message.label, "disclosure", message.body) {
                            onSelect(MessageContact(message.peer, message.label))
                        } }))
                    }
                }
            }
            form(NativeFormModel(sections = sections), "screen", Modifier.fillMaxSize())
            return
        }
        val title = if (managing) "Blocked accounts" else if (requests) "Requests" else "Inbox"
        val goBack = { if (nativePage != null) onClose() else if (managing) managing = false else if (requests) requests = false else onClose() }
        val settingsMenu: @Composable () -> Unit = {
            if (!managing && !requests) Box {
                IconButton(onClick = { options = true }) { LineIcon("more", "Inbox settings") }
                DropdownMenu(expanded = options, onDismissRequest = { options = false }) {
                    DropdownMenuItem(text = { Text("Requests") }, onClick = { options = false; if (nativePage != null) openPage(InboxPage.Requests) else requests = true })
                    DropdownMenuItem(text = { Text("Privacy") }, onClick = { options = false; onPrivacy() })
                    DropdownMenuItem(text = { Text("Blocked accounts") }, onClick = { options = false; if (nativePage != null) openPage(InboxPage.Blocked) else { managing = true; onRefresh() } })
                }
            }
        }
        Column(Modifier.fillMaxSize()) {
            if (navigation != null) navigation.UpBar(title, goBack) { settingsMenu() }
            LazyColumn(Modifier.fillMaxSize(), contentPadding = PaddingValues(horizontal = 22.dp, vertical = 12.dp)) {
                if (nativePage == null && navigation == null) item {
                    Row(Modifier.fillMaxWidth(), verticalAlignment = Alignment.CenterVertically) {
                        Text(title, Modifier.weight(1f), style = MaterialTheme.typography.headlineMedium)
                        settingsMenu()
                        TextButton(onClick = goBack) { Text(if (managing || requests) "Back" else "Done") }
                    }
                }
                if (managing) {
                    if (blocks.isEmpty()) item { Text("You haven’t blocked anyone.") }
                    items(blocks, key = { it.id }) { block ->
                        Row(verticalAlignment = Alignment.CenterVertically) {
                            Text(block.label, Modifier.weight(1f))
                            TextButton(onClick = { onUnblock(block.id) }) { Text("Unblock") }
                        }
                    }
                } else if (requests) {
                    item { Text(if (connection.requests.isEmpty()) "No message requests." else "Approve a sender to read their messages.",
                        style = MaterialTheme.typography.bodyMedium, color = MaterialTheme.colorScheme.onSurfaceVariant) }
                    items(connection.requests, key = { it }) { peer ->
                        val label = connection.snapshot.posts.firstOrNull { it.visibility?.peer == peer }?.author
                        Column(Modifier.fillMaxWidth().padding(vertical = 12.dp)) {
                            Text(label ?: "Account ${peer.take(12)}", style = MaterialTheme.typography.titleMedium)
                            Row(horizontalArrangement = Arrangement.spacedBy(12.dp)) {
                                FilledTonalButton(onClick = { onApprove(peer) }) { Text("Accept") }
                                if (onReport != null) TextButton(onClick = { onReport(ReportTarget("account",peer)) }) { Text("Report account") }
                                TextButton(onClick = { onBlock(MessageContact(peer,label ?: "Account ${peer.take(12)}")) }) { Text("Block") }
                            }
                        }
                        HorizontalDivider()
                    }
                } else {
                    item {
                        if (connection.requests.isNotEmpty()) TextButton(onClick = { if (nativePage == null) requests = true else openPage(InboxPage.Requests) }) {
                            Text("Message requests (${connection.requests.size})")
                        }
                        inboxStatus?.let { Text(it, style = MaterialTheme.typography.bodySmall, color = MaterialTheme.colorScheme.onSurfaceVariant) }
                    }
                    if (recent.isEmpty()) item {
                        Column(Modifier.fillMaxWidth().padding(horizontal = 12.dp, vertical = 48.dp),
                            horizontalAlignment = Alignment.CenterHorizontally, verticalArrangement = Arrangement.spacedBy(12.dp)) {
                            EmptyStateEmblem(if (navigation != null) "inbox" else "reply")
                            Text(emptyTitle, style = MaterialTheme.typography.headlineSmall, fontWeight = FontWeight.Bold)
                            Text(emptyDetail, textAlign = androidx.compose.ui.text.style.TextAlign.Center, color = MaterialTheme.colorScheme.onSurfaceVariant)
                            FilledTonalButton(onClick = if (connection.enabled) onBrowse else onJoin) {
                                Text(if (connection.enabled) "Browse posts" else "Community settings")
                            }
                        }
                    }
                    items(recent, key = { it.peer }) { m ->
                        Column(Modifier.fillMaxWidth().clickable { onSelect(MessageContact(m.peer, m.label)) }.padding(vertical = 14.dp)) {
                            Text(m.label, fontWeight = FontWeight.SemiBold)
                            Text(m.body, maxLines = 2)
                        }
                        HorizontalDivider()
                    }
                }
            }
        }
        return
    }
    val lines = connection.messages.filter { it.peer == contact.peer }
    var draft by remember(contact.peer) { mutableStateOf("") }
    var sending by remember(contact.peer) { mutableStateOf(false) }
    val list = rememberLazyListState()
    val scope = rememberCoroutineScope()
    val nativeEditor = LocalInlineEditor.current
    fun canSend() = !sending && draft.isNotBlank() && connection.directAvailable
    fun sendMessage() {
        if (!canSend()) return
        val message = draft
        sending = true
        scope.launch {
            try { if (onSend(contact, message)) draft = "" } finally { sending = false }
        }
    }
    LaunchedEffect(contact.peer, lines.lastOrNull()?.id) { if (lines.isNotEmpty()) list.scrollToItem(lines.lastIndex) }
    Column(Modifier.fillMaxSize().imePadding()) {
        if (navigation != null) navigation.UpBar(contact.label, { onSelect(null) }) {
            TextButton(onClick = { onBlock(contact) }) { Text("Block") }
        }
        Column(Modifier.fillMaxSize().padding(horizontal = 16.dp)) {
            if (navigation == null) Row(verticalAlignment = Alignment.CenterVertically) {
                if (nativePage == null) TextButton(onClick = { onSelect(null) }) { Text("Inbox") }
                if (nativePage == null) Text(contact.label, Modifier.weight(1f), style = MaterialTheme.typography.titleMedium)
                else Spacer(Modifier.weight(1f))
                TextButton(onClick = { onBlock(contact) }) { Text("Block") }
            }
            Text("End-to-end encrypted", color = MaterialTheme.colorScheme.primary, style = MaterialTheme.typography.labelMedium)
            connection.messageStatus?.let { Text(it, color = MaterialTheme.colorScheme.error) }
            LazyColumn(Modifier.weight(1f).fillMaxWidth(), state = list, contentPadding = PaddingValues(vertical = 12.dp), verticalArrangement = Arrangement.spacedBy(10.dp)) {
                if (lines.isEmpty() && !connection.mailboxElsewhere) item { Text("Send your first message to ${contact.label}.", color = MaterialTheme.colorScheme.onSurfaceVariant) }
                items(lines, key = { it.id }) { m ->
                    Column(Modifier.fillMaxWidth(), horizontalAlignment = if (m.outgoing) Alignment.End else Alignment.Start) {
                        Surface(shape = MaterialTheme.shapes.medium, color = if (m.outgoing) MaterialTheme.colorScheme.primaryContainer else MaterialTheme.colorScheme.surfaceVariant) {
                            Text(m.body, Modifier.padding(12.dp))
                        }
                        if (!m.outgoing && onReport != null) {
                            val report = { onReport(ReportTarget("message", m.peer, m.body)) }
                            val control = LocalFeedControl.current
                            if (control != null) control(FormRow("report-message", "Report message", onTap = report), Modifier)
                            else TextButton(onClick = report) { Text("Report message") }
                        }
                        if (m.outgoing) Text(when (m.status) { "queued" -> "Waiting to send"; "sent" -> "Sent"; else -> "Couldn’t deliver" }, style = MaterialTheme.typography.labelSmall)
                    }
                }
            }
            if (connection.mailboxElsewhere) return@Column
            if (nativeEditor != null) {
                nativeEditor(InlineEditorModel(
                    currentBody = { draft }, limit = 1000, isSending = { sending }, sendEnabled = ::canSend,
                    onEdit = { draft = it }, onSend = ::sendMessage,
                ))
            } else {
                OutlinedTextField(value = draft, onValueChange = { if (!sending && it.length <= 1000) draft = it },
                    label = { Text("Message") }, modifier = Modifier.fillMaxWidth(), maxLines = 4, readOnly = sending)
                YapButton(modifier = Modifier.align(Alignment.End), enabled = canSend(), onClick = ::sendMessage) { Text("Send") }
            }
        }
    }
}
