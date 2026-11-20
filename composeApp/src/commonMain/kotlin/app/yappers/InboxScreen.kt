package app.yappers

import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.lazy.itemsIndexed
import androidx.compose.foundation.lazy.rememberLazyListState
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.dp
import androidx.compose.foundation.background
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.ui.text.style.TextOverflow
import kotlin.time.Clock
import kotlinx.coroutines.delay
import kotlinx.coroutines.launch

class ChatBubble(val text: String, val sender: String, val outgoing: Boolean, val endsGroup: Boolean,
                 private val onReport: (() -> Unit)?) {
    val canReport: Boolean get() = onReport != null
    fun report() { onReport?.invoke() }
}

internal val LocalChatBubble = staticCompositionLocalOf<(@Composable (ChatBubble, Modifier) -> Unit)?> { null }

private fun inboxTime(timestamp: Long, now: Long): String {
    if (timestamp <= 0) return ""
    val seconds = (now - timestamp).coerceAtLeast(0)
    fun rounded(unit: Long) = ((seconds + unit / 2) / unit).coerceAtLeast(1)
    val day = 86_400L
    val year = 31_557_600L
    return when {
        seconds < 45 -> "Just now"
        seconds + 30 < 3600 -> "${rounded(60)}m"
        seconds + 1800 < day -> "${rounded(3600)}h"
        seconds + day / 2 < 14 * day -> "${rounded(day)}d"
        seconds + 7 * day / 2 < 56 * day -> "${rounded(7 * day)}w"
        seconds + year / 24 < year -> "${rounded(year / 12)}mo"
        else -> "${rounded(year)}y"
    }
}

private fun InboxItem.unreadLabel() = when (unread) {
    0 -> ""
    1 -> "1 new reply"
    else -> "$unread new replies"
}

@Composable
private fun ActivityRow(item: InboxItem, now: Long, open: () -> Unit) {
    val palette = MaterialTheme.colorScheme
    Row(Modifier.fillMaxWidth().clickable(onClick = open).padding(vertical = 15.dp),
        horizontalArrangement = Arrangement.spacedBy(12.dp)) {
        if (item.anonymous || item.byline.isBlank()) {
            Box(Modifier.size(42.dp).background(palette.secondaryContainer, CircleShape), contentAlignment = Alignment.Center) {
                LineIcon("reply", null, Modifier.size(21.dp), tint = palette.onSecondaryContainer)
            }
        } else ProfileAvatar(item.byline, item.photo?.let { CommunityProfile(photo = it) }, 42.dp)
        Column(Modifier.weight(1f), verticalArrangement = Arrangement.spacedBy(4.dp)) {
            Row(verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                Text("${item.byline.ifBlank { "A neighbor" }} replied", Modifier.weight(1f),
                    style = MaterialTheme.typography.titleSmall, fontWeight = FontWeight.SemiBold)
                Text(inboxTime(item.created, now), style = MaterialTheme.typography.labelSmall, color = palette.onSurfaceVariant)
            }
            if (item.preview.isNotBlank()) Text(item.preview, style = MaterialTheme.typography.bodyMedium,
                maxLines = 2, overflow = TextOverflow.Ellipsis)
            Row(verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(5.dp)) {
                LineIcon("reply", null, Modifier.size(14.dp), tint = palette.onSurfaceVariant)
                Text(item.title, style = MaterialTheme.typography.bodySmall, color = palette.onSurfaceVariant,
                    maxLines = 1, overflow = TextOverflow.Ellipsis)
            }
            if (item.unread > 0) Row(verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(6.dp)) {
                Box(Modifier.size(6.dp).background(palette.primary, CircleShape))
                Text(item.unreadLabel(), style = MaterialTheme.typography.labelSmall, color = palette.primary)
            }
        }
    }
}

enum class InboxPage { Messages, Requests, Blocked, Privacy, Conversation }

@Composable
internal fun InboxScreen(
    connection: NetkitState, contact: MessageContact?, blocks: List<BlockedAccount>,
    onPrivacy: () -> Unit, onApprove: (String) -> Unit,
    onSelect: (MessageContact?) -> Unit, onClose: () -> Unit, onBrowse: () -> Unit, onJoin: () -> Unit, onRefresh: () -> Unit,
    onReport: ((ReportTarget) -> Unit)?, onBlock: (MessageContact) -> Unit, onUnblock: (String) -> Unit,
    onActivity: (InboxItem) -> Unit, onRead: (InboxItem) -> Unit, onDismissRequest: (String) -> Unit,
    pushBusy: Boolean, onPush: (Set<String>) -> Unit,
    nativePage: InboxPage? = null, openPage: (InboxPage) -> Unit = {},
    onDownload: suspend (Attachment) -> List<String>,
    onSend: suspend (MessageContact, String, SelectedAttachment?) -> Boolean
) {
    val inbox = connection.privacy.notifications
    var activity by remember { mutableStateOf(inbox.activity.any { it.unread > 0 } &&
        inbox.messages.none { it.unread > 0 } && inbox.requests.isEmpty()) }
    val now by produceState(Clock.System.now().epochSeconds) {
        while (true) { delay(30_000); value = Clock.System.now().epochSeconds }
    }
    var showDelivery by remember { mutableStateOf(false) }
    val activityItems = inbox.activity.sortedByDescending { it.unread > 0 }
    val photos = remember(connection.snapshot.posts) { inboxPhotos(connection.snapshot.posts) }
    val current = inbox.messages.firstOrNull { it.target == contact?.peer && it.unread > 0 }
    val visible = current != null && connection.messages.any { it.id == current.last && it.peer == current.target }
    LaunchedEffect(current?.through, visible) { if (current != null && visible) onRead(current) }
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
                            val elapsed = inboxTime(inbox.requests.firstOrNull { it.target == peer }?.created ?: 0, now)
                            add(FormSection(listOf(label,elapsed).filter(String::isNotBlank).joinToString(" · "), buildList {
                                add(FormRow("accept", "Accept") { onApprove(peer) })
                                add(FormRow("dismiss", "Dismiss") { onDismissRequest(peer) })
                                if (onReport != null) add(FormRow("report", "Report account") { onReport(ReportTarget("account", peer)) })
                                add(FormRow("block", "Block", destructive = true) { onBlock(MessageContact(peer, label)) })
                            }))
                        }
                    }
                    else -> {
                        add(FormSection(rows = listOf(FormRow("folder", "Inbox folders", "segments",
                            value = if (activity) "Activity" else "Messages",
                            choices = listOf(FormChoice("Activity", activity) { activity = true },
                                FormChoice("Messages", !activity) { activity = false })))))
                        if (activity) {
                            if (activityItems.isEmpty()) add(FormSection(rows = listOf(FormRow("empty", "No replies yet", "text",
                                "When someone replies to one of your posts, you’ll see their reply here."))))
                            else for ((unread, rows) in activityItems.groupBy { it.unread > 0 }) {
                                val section = if (activityItems.any { it.unread == 0 } && activityItems.any { it.unread > 0 })
                                    if (unread) "Unread" else "Earlier" else ""
                                add(FormSection(section, rows.map { item ->
                                    val caption = listOf("On ${item.title}", inboxTime(item.created,now), item.unreadLabel())
                                        .filter(String::isNotBlank).joinToString(" · ")
                                    FormRow(item.target, "${item.byline.ifBlank { "A neighbor" }} replied", "activity",
                                        item.preview, value = item.photo.orEmpty(), checked = unread, caption = caption,
                                        symbol = if (item.anonymous || item.byline.isBlank()) "bubble.left.fill" else "person.crop.circle.fill",
                                        onTap = { onActivity(item) })
                                }))
                            }
                        } else {
                        if (connection.requests.isNotEmpty()) add(FormSection(rows = listOf(
                            FormRow("requests", "Message requests (${connection.requests.size})", "disclosure", inboxTime(inbox.requests.firstOrNull()?.created ?: 0, now)) { openPage(InboxPage.Requests) })))
                        inboxStatus?.let { add(FormSection(rows = listOf(FormRow("status", it, "text")))) }
                        if (recent.isEmpty()) {
                            add(FormSection(rows = listOf(FormRow("empty", emptyTitle, "text", emptyDetail),
                                FormRow("browse", if (connection.enabled) "Browse posts" else "Community settings", onTap = if (connection.enabled) onBrowse else onJoin))))
                        } else {
                            add(FormSection(rows = recent.map { message ->
                                FormRow(message.peer, message.label + if(inbox.messages.any { it.target==message.peer && it.unread>0 }) " · Unread" else "", "conversation", (if (message.outgoing) "You: " else "") + message.body, value = photos[message.peer].orEmpty(),
                                    caption = inboxTime(message.timestamp,now)) {
                                    onSelect(MessageContact(message.peer, message.label))
                                }
                            }))
                        }
                        }
                        if (connection.privacy.pushAvailable) {
                            add(FormSection(rows = listOf(FormRow("delivery", "Device notifications", "disclosure", onTap = { showDelivery = !showDelivery }))))
                        if (showDelivery) add(FormSection("Notify me about", listOf("reply" to "Replies to my posts", "message" to "Direct messages", "request" to "Message requests").map { (kind,label) ->
                            val categories=connection.privacy.pushCategories
                            FormRow("push-$kind", label, "toggle", enabled = !pushBusy, checked = kind in categories,
                                onToggle = { enabled -> onPush(if (enabled) categories + kind else categories - kind) })
                        }))
                        }
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
                if (!managing && !requests) item {
                    Row {
                        FilterChip(selected=activity,onClick={activity=true},label={Text("Activity")})
                        Spacer(Modifier.width(8.dp))
                        FilterChip(selected=!activity,onClick={activity=false},label={Text("Messages")})
                    }
                }
                if (activity && !managing && !requests) {
                    if (activityItems.isEmpty()) item {
                        Column(Modifier.fillMaxWidth().padding(24.dp), horizontalAlignment = Alignment.CenterHorizontally,
                            verticalArrangement = Arrangement.spacedBy(10.dp)) {
                            EmptyStateEmblem("reply")
                            Text("No replies yet", style = MaterialTheme.typography.titleLarge)
                            Text("When someone replies to one of your posts, you’ll see their reply here.",
                                style = MaterialTheme.typography.bodyMedium, color = MaterialTheme.colorScheme.onSurfaceVariant,
                                textAlign = androidx.compose.ui.text.style.TextAlign.Center)
                        }
                    }
                    for ((unread, rows) in activityItems.groupBy { it.unread > 0 }) {
                        if (activityItems.any { it.unread == 0 } && activityItems.any { it.unread > 0 }) item {
                            Text(if (unread) "Unread" else "Earlier", Modifier.padding(top=16.dp),
                                style = MaterialTheme.typography.labelLarge, color = MaterialTheme.colorScheme.onSurfaceVariant)
                        }
                        items(rows, key={it.target}) { row ->
                            ActivityRow(row, now) { onActivity(row) }
                            HorizontalDivider()
                        }
                    }
                } else if (managing) {
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
                            Row(verticalAlignment = Alignment.CenterVertically) {
                                Text(label ?: "Account ${peer.take(12)}", Modifier.weight(1f), style = MaterialTheme.typography.titleMedium)
                                Text(inboxTime(inbox.requests.firstOrNull { it.target == peer }?.created ?: 0, now),
                                    style = MaterialTheme.typography.labelSmall, color = MaterialTheme.colorScheme.onSurfaceVariant)
                            }
                            Row(horizontalArrangement = Arrangement.spacedBy(12.dp)) {
                                FilledTonalButton(onClick = { onApprove(peer) }) { Text("Accept") }
                                TextButton(onClick={onDismissRequest(peer)}) { Text("Dismiss") }
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
                        Row(Modifier.fillMaxWidth().clickable { onSelect(MessageContact(m.peer, m.label)) }.padding(vertical=14.dp),
                            horizontalArrangement = Arrangement.spacedBy(12.dp)) {
                            ProfileAvatar(m.label, photos[m.peer]?.let { CommunityProfile(photo = it) }, 42.dp)
                            Column(Modifier.weight(1f)) {
                                Row(verticalAlignment = Alignment.CenterVertically) {
                                    Text(m.label, Modifier.weight(1f), style=MaterialTheme.typography.titleSmall, fontWeight=FontWeight.SemiBold)
                                    Text(inboxTime(m.timestamp, now), style=MaterialTheme.typography.labelSmall, color=MaterialTheme.colorScheme.onSurfaceVariant)
                                }
                                Text((if (m.outgoing) "You: " else "") + m.body, maxLines = 2, overflow = TextOverflow.Ellipsis,
                                    style=MaterialTheme.typography.bodyMedium)
                                val unread = inbox.messages.firstOrNull { it.target == m.peer }?.unread ?: 0
                                if (unread > 0) Text("$unread unread", style=MaterialTheme.typography.labelSmall, color=MaterialTheme.colorScheme.primary)
                            }
                        }
                        HorizontalDivider()
                    }
                }
                    if (connection.privacy.pushAvailable && !managing && !requests) item {
                        TextButton(onClick = {showDelivery = !showDelivery}) { Text("Device notifications") }
                        if (showDelivery)
                        listOf("reply" to "Replies to my posts","message" to "Direct messages","request" to "Message requests").forEach { (kind,label) ->
                            val categories=connection.privacy.pushCategories
                            Row(verticalAlignment=Alignment.CenterVertically) {
                                Checkbox(enabled=!pushBusy,checked=kind in categories,onCheckedChange={onPush(if(it) categories+kind else categories-kind)})
                                Text(label)
                            }
                        }
                    }
            }
        }
        return
    }
    val lines = connection.messages.filter { it.peer == contact.peer }
    var draft by remember(contact.peer) { mutableStateOf("") }
    var file by remember(contact.peer) { mutableStateOf<SelectedAttachment?>(null) }
    var fileError by remember(contact.peer) { mutableStateOf<String?>(null) }
    var sending by remember(contact.peer) { mutableStateOf(false) }
    val list = rememberLazyListState()
    val scope = rememberCoroutineScope()
    val bubble = LocalChatBubble.current
    val nativeEditor = LocalInlineEditor.current
    val nativeControl = LocalFeedControl.current
    val files = LocalAttachmentFiles.current
    fun canSend() = !sending && (draft.isNotBlank() || file != null) && connection.directAvailable
    fun pickFile() {
        if (file != null) { file = null; fileError = null; return }
        files?.chooseAttachment { selected, error ->
            fileError = error
            if (selected != null) file = selected
        }
    }
    fun sendMessage() {
        if (!canSend()) return
        val message = draft
        sending = true
        scope.launch {
            try {
                if (onSend(contact, message, file)) { draft = ""; file = null; fileError = null }
            } finally { sending = false }
        }
    }
    LaunchedEffect(contact.peer, lines.lastOrNull()?.id) { if (lines.isNotEmpty()) list.scrollToItem(lines.lastIndex) }
    Column(Modifier.fillMaxSize().imePadding()) {
        if (navigation != null) navigation.UpBar(contact.label, { onSelect(null) }) {
            TextButton(onClick = { onBlock(contact) }) { Text("Block") }
        }
        Column(Modifier.fillMaxSize().padding(horizontal = 16.dp)) {
            if (nativeControl != null) {
                Row(Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.SpaceBetween, verticalAlignment = Alignment.CenterVertically) {
                    Text("End-to-end encrypted", Modifier.weight(1f), color = MaterialTheme.colorScheme.onSurfaceVariant,
                        style = MaterialTheme.typography.bodySmall)
                    nativeControl(FormRow("block", "Block", destructive = true) { onBlock(contact) }, Modifier)
                }
            } else {
                if (navigation == null) Row(verticalAlignment = Alignment.CenterVertically) {
                    if (nativePage == null) TextButton(onClick = { onSelect(null) }) { Text("Inbox") }
                    if (nativePage == null) Text(contact.label, Modifier.weight(1f), style = MaterialTheme.typography.titleMedium)
                    else Spacer(Modifier.weight(1f))
                    TextButton(onClick = { onBlock(contact) }) { Text("Block") }
                }
                Text("End-to-end encrypted", color = MaterialTheme.colorScheme.primary, style = MaterialTheme.typography.labelMedium)
            }
            connection.messageStatus?.let { Text(it, color = MaterialTheme.colorScheme.error) }
            LazyColumn(Modifier.weight(1f).fillMaxWidth(), state = list, contentPadding = PaddingValues(vertical = 12.dp), verticalArrangement = Arrangement.spacedBy(if (bubble == null) 10.dp else 3.dp)) {
                if (lines.isEmpty() && !connection.mailboxElsewhere) item { Text("Send your first message to ${contact.label}.", color = MaterialTheme.colorScheme.onSurfaceVariant) }
                itemsIndexed(lines, key = { _, message -> message.id }) { index, m ->
                    val endsGroup = lines.getOrNull(index + 1)?.outgoing != m.outgoing
                    val startsGroup = index > 0 && lines[index - 1].outgoing != m.outgoing
                    val top = if (bubble != null && startsGroup) 6.dp else 0.dp
                    Column(Modifier.fillMaxWidth().padding(top = top), horizontalAlignment = if (m.outgoing) Alignment.End else Alignment.Start) {
                        if (bubble != null && m.body.isNotEmpty()) {
                            val report = onReport?.takeUnless { m.outgoing }?.let { reportMessage ->
                                { reportMessage(ReportTarget("message", m.peer, m.body)) }
                            }
                            BoxWithConstraints(Modifier.fillMaxWidth(), contentAlignment = if (m.outgoing) Alignment.CenterEnd else Alignment.CenterStart) {
                                bubble(ChatBubble(m.body, if (m.outgoing) "From you" else "From ${contact.label}", m.outgoing, endsGroup, report),
                                    Modifier.widthIn(max = maxWidth * 0.82f))
                            }
                        } else if (bubble == null && m.body.isNotEmpty()) {
                            Surface(shape = MaterialTheme.shapes.medium, color = if (m.outgoing) MaterialTheme.colorScheme.primaryContainer else MaterialTheme.colorScheme.surfaceVariant) {
                                Text(m.body, Modifier.padding(12.dp))
                            }
                            if (!m.outgoing && onReport != null) TextButton(onClick = { onReport(ReportTarget("message", m.peer, m.body)) }) { Text("Report message") }
                        }
                        m.attachment?.let { AttachmentDownload(it, onDownload) }
                        if (m.outgoing && (bubble == null || endsGroup || m.status != "sent")) Text(when (m.status) { "queued" -> "Waiting to send"; "sent" -> "Sent"; else -> "Couldn’t deliver" },
                            modifier = if (bubble != null) Modifier.padding(top = 3.dp, end = 6.dp) else Modifier,
                            style = MaterialTheme.typography.labelSmall)
                    }
                }
            }
            if (connection.mailboxElsewhere) return@Column
            if (files != null) {
                Row(verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                    file?.let { Text(it.name, Modifier.weight(1f), maxLines = 2, overflow = TextOverflow.Ellipsis) }
                    val label = if (file == null) "Attach file" else "Remove file"
                    val enabled = !sending && connection.directAvailable
                    if (nativeControl != null) nativeControl(FormRow("attach", label, symbol = "paperclip", enabled = enabled, onTap = ::pickFile), Modifier)
                    else TextButton(enabled = enabled, onClick = ::pickFile) { Text(label) }
                }
                fileError?.let { Text(it, color = MaterialTheme.colorScheme.error) }
            }
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


// Profile snapshots belong to named posts. A newer snapshot with an empty photo clears the older one.
internal fun inboxPhotos(posts: List<Post>): Map<String, String> {
    val latest = mutableMapOf<String, Post>()
    for (post in posts) {
        val visibility = post.visibility ?: continue
        val peer = visibility.peer ?: continue
        if (visibility.anonymous || post.hidden || post.profile == null) continue
        val previous = latest[peer]
        if (previous == null || (post.acceptedAt ?: 0) > (previous.acceptedAt ?: 0)) latest[peer] = post
    }
    return latest.mapValues { (_, post) -> post.profile?.photo.orEmpty() }
}
