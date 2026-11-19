@file:OptIn(androidx.compose.material3.ExperimentalMaterial3Api::class)

package app.yappers

import androidx.compose.foundation.layout.*
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.rememberLazyListState
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.runtime.retain.retain
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalClipboardManager
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.AnnotatedString
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.dp
import kotlinx.coroutines.CancellationException
import kotlinx.coroutines.launch
import kotlinx.serialization.Serializable
import kotlinx.serialization.json.*

@Serializable
private data class PrivateGroupSummary(val id: String, val name: String)

@Serializable
private data class PrivatePost(
    val id: String, val author: String, val headline: String? = null,
    val body: String, val parent: String? = null
)

@Serializable
private data class PrivateGroupPage(
    val name: String, val owner: Boolean, val phase: String,
    val posts: List<PrivatePost>, val pending: Boolean,
    val before: Long? = null, val joinRequest: String? = null, val invitation: String? = null,
    val members: Map<String, JsonObject>? = null
)

private val privateGroupJson = Json { ignoreUnknownKeys = true }

@Composable
internal fun PrivateGroupsScreen(controller: NetkitController, alias: String, onBack: () -> Unit) {
    val scope = rememberCoroutineScope()
    val materialNavigation = LocalMaterialNavigation.current
    @Suppress("DEPRECATION") val clipboard = LocalClipboardManager.current
    var groups by remember { mutableStateOf(emptyList<PrivateGroupSummary>()) }
    var group by retain { mutableStateOf<String?>(null) }
    var page by retain { mutableStateOf<PrivateGroupPage?>(null) }
    var editor by retain { mutableStateOf<String?>(null) }
    var input by retain { mutableStateOf("") }
    var heading by retain { mutableStateOf("") }
    var reply by retain { mutableStateOf<PrivatePost?>(null) }
    var busy by remember { mutableStateOf(false) }
    var notice by remember { mutableStateOf("") }
    var discardEditor by remember { mutableStateOf(false) }
    var confirmDiscard by remember { mutableStateOf(false) }
    var removeDevice by remember { mutableStateOf<String?>(null) }

    suspend fun request(kind: String, vararg fields: Pair<String, JsonElement>) =
        controller.privateGroupAction(buildJsonObject {
            put("kind", kind)
            fields.forEach { (key, value) -> put(key, value) }
        })
    suspend fun list() {
        groups = privateGroupJson.decodeFromJsonElement(request("list"))
    }
    suspend fun open(id: String, before: Long? = null) {
        val next = privateGroupJson.decodeFromJsonElement<PrivateGroupPage>(request("open",
            "group" to JsonPrimitive(id), "before" to (before?.let(::JsonPrimitive) ?: JsonNull)))
        val previous = page
        page = if (before != null && next.phase == "joined" && previous != null)
            next.copy(posts = (previous.posts + next.posts).distinctBy { it.id }) else next
        group = id
    }
    fun act(action: suspend () -> Unit) {
        if (busy) return
        busy = true
        notice = ""
        scope.launch {
            try { action() }
            catch (failure: Exception) {
                if (failure is CancellationException) throw failure
                notice = failure.message ?: "Couldn’t update this private group."
            } finally { busy = false }
        }
    }
    fun begin(kind: String, parent: PrivatePost? = null) {
        editor = kind
        input = ""
        heading = ""
        reply = parent
        notice = ""
    }
    fun dismissEditor() {
        if (busy) return
        if (input.isNotBlank() || heading.isNotBlank()) discardEditor = true
        else editor = null
    }
    fun back() {
        if (busy) return
        notice = ""
        when {
            editor != null -> dismissEditor()
            group != null -> { group = null; page = null }
            else -> onBack()
        }
    }
    fun copy(value: String, label: String) {
        clipboard.setText(AnnotatedString(value))
        notice = label
    }
    LaunchedEffect(Unit) { act { list() } }
    AppBackHandler(materialNavigation == null || group != null, ::back)

    val current = page
    val id = group
    val fields = editor
    fun editorSection(fields: String): FormSection {
        val isPost = fields == "New post"
        val isReply = fields == "Write a reply"
        val isMessage = isPost || isReply
        val limit = when {
            fields == "Create private group" -> 60
            isMessage -> 3000
            else -> 4096
        }
        val label = when (fields) {
            "Create private group" -> "Group name"
            "Open invitation" -> "Paste invitation"
            "Approve a device" -> "Paste signed join request"
            "New post" -> "Post body"
            else -> "Reply"
        }
        val valid = input.isNotBlank() && (!isPost || heading.isNotBlank())
        return FormSection(rows = buildList {
            if (isPost) add(FormRow("headline", "Headline", kind = "field", value = heading, limit = 100,
                enabled = !busy, onChange = { heading = it.take(100) }))
            if (isReply) add(FormRow("parent", "Replying to ${reply?.headline ?: "a post"}", kind = "text"))
            add(FormRow("input", label, kind = if (limit == 60) "field" else "multiline", value = input,
                limit = limit, enabled = !busy, keyboard = if (limit == 4096) "ascii" else "",
                onChange = { input = it.take(limit) }))
            add(FormRow("submit", when {
                isPost -> "Post"
                isReply -> "Send reply"
                fields == "Approve a device" -> "Approve device"
                else -> fields
            }, enabled = valid && !busy, onTap = {
                act {
                    when (fields) {
                        "Create private group", "Open invitation" -> {
                            val kind = if (fields == "Open invitation") "import" else "create"
                            val key = if (kind == "import") "invitation" else "name"
                            val opened = request(kind, key to JsonPrimitive(input.trim())).jsonPrimitive.content
                            list()
                            editor = null
                            open(opened)
                        }
                        "Approve a device" -> {
                            val target = checkNotNull(id)
                            request("approve", "group" to JsonPrimitive(target), "request" to JsonPrimitive(input.trim()))
                            editor = null
                            open(target)
                        }
                        else -> {
                            val target = checkNotNull(id)
                            val draft = buildJsonObject {
                                put("author", alias)
                                put("headline", if (isPost) JsonPrimitive(heading.trim()) else JsonNull)
                                put("body", input.trim())
                                put("parent", reply?.id?.let(::JsonPrimitive) ?: JsonNull)
                            }
                            request("prepare", "group" to JsonPrimitive(target), "draft" to draft)
                            page = page?.copy(pending = true)
                            editor = null
                            input = ""
                            heading = ""
                            reply = null
                            request("publish", "group" to JsonPrimitive(target))
                            page = page?.copy(pending = false)
                            open(target)
                        }
                    }
                }
            }))
        }, footer = when (fields) {
            "Approve a device" -> "Approve only a request you received directly from the person you invited."
            "Open invitation" -> "The owner will need to approve this device before you can read or post."
            else -> ""
        })
    }

    val sections = buildList {
        add(FormSection(rows = buildList {
            if (materialNavigation == null) {
                add(FormRow("back", if (id == null && fields == null) "All groups" else "Back",
                    enabled = !busy, onTap = ::back))
                add(FormRow("title", fields ?: current?.name ?: "Private groups", kind = "text"))
            }
        }, footer = if (materialNavigation == null) notice.ifBlank { if (busy) "Working…" else "" } else ""))

        if (fields != null && materialNavigation == null) {
            add(editorSection(fields))
        } else if (id == null || current == null) {
            add(FormSection(rows = buildList {
                add(FormRow("create", "Create private group", symbol = "plus", enabled = !busy,
                    onTap = { begin("Create private group") }))
                add(FormRow("import", "Open invitation", enabled = !busy, onTap = { begin("Open invitation") }))
                groups.sortedBy { it.name.lowercase() }.forEach { group ->
                    add(FormRow(group.id, group.name, kind = "disclosure", enabled = !busy, onTap = { act { open(group.id) } }))
                }
            }))
        } else {
            add(FormSection(rows = buildList {
                if (materialNavigation == null) {
                    add(FormRow("refresh", "Refresh group", enabled = !busy, onTap = { act { open(id) } }))
                }
                current.joinRequest?.let { code ->
                    add(FormRow("join-request", "Copy join request", enabled = !busy,
                        onTap = { copy(code, "Join request copied.") }))
                }
                current.invitation?.let { invitation ->
                    add(FormRow("share", "Copy invitation", enabled = !busy,
                        onTap = { copy(invitation, "Invitation copied.") }))
                    add(FormRow("approve", "Approve a device", enabled = !busy,
                        onTap = { begin("Approve a device") }))
                }
                if (current.phase == "joined" && materialNavigation == null) {
                    add(FormRow("post", "New post", symbol = "square.and.pencil", enabled = !busy && !current.pending,
                        onTap = { begin("New post") }))
                }
                if (current.pending) {
                    add(FormRow("retry", "Retry pending post", enabled = !busy && current.phase == "joined", onTap = {
                        act {
                            request("prepare", "group" to JsonPrimitive(id))
                            request("publish", "group" to JsonPrimitive(id))
                            page = page?.copy(pending = false)
                            open(id)
                        }
                    }))
                    add(FormRow("discard", "Discard pending post", destructive = true, enabled = !busy,
                        onTap = { confirmDiscard = true }))
                }
            }, footer = when (current.phase) {
                "pending" -> "Send your join request to the owner. Your history starts when they approve it."
                "removed" -> "This device no longer has access."
                else -> "Posts and replies are end-to-end encrypted."
            }))
            if (current.phase == "joined") {
                val byParent = current.posts.groupBy { it.parent }
                val roots = current.posts.filter { it.parent == null }
                val rootIds = roots.map { it.id }.toSet()
                val orphaned = current.posts.filter { it.parent != null && it.parent !in rootIds }
                if (current.posts.isEmpty()) add(FormSection(rows = listOf(FormRow("empty", "No posts since you joined", kind = "text"))))
                (roots + orphaned).forEach { post ->
                    add(FormSection(rows = buildList {
                        add(FormRow(post.id, post.headline ?: "${post.author} · Reply to an earlier post",
                            kind = "text", detail = if (post.parent == null) "${post.author}\n${post.body}" else post.body))
                        byParent[post.id].orEmpty().asReversed().forEach { comment ->
                            add(FormRow(comment.id, comment.author, kind = "text", detail = comment.body))
                        }
                        if (post.parent == null) add(FormRow("reply-${post.id}", "Reply", enabled = !busy && !current.pending,
                            onTap = { begin("Write a reply", post) }))
                    }))
                }
                current.before?.let { before ->
                    add(FormSection(rows = listOf(FormRow("older", "Load older posts", enabled = !busy,
                        onTap = { act { open(id, before) } }))))
                }
                if (current.owner) current.members?.keys?.filterNot { key ->
                    current.members[key]?.get("since")?.jsonPrimitive?.intOrNull == 1
                }?.takeIf { it.isNotEmpty() }?.let { members ->
                    add(FormSection(title = "Member devices", rows = members.map { member ->
                        FormRow(member, "Remove device ${member.take(12)}", destructive = true, enabled = !busy,
                            onTap = { removeDevice = member })
                    }))
                }
            }
        }
    }
    val native = LocalNativeForm.current
    if (native != null) {
        native(NativeFormModel(sections = sections), "screen", Modifier.fillMaxSize())
    } else {
        val snackbar = remember { SnackbarHostState() }
        LaunchedEffect(notice, fields, materialNavigation) {
            if (materialNavigation != null && notice.isNotBlank() && fields == null) {
                snackbar.showSnackbar(notice)
            }
        }
        val postsById = current?.posts.orEmpty().associateBy { it.id }
        val list = key(id) { rememberLazyListState() }
        val expandedPostAction by remember(list) { derivedStateOf { !list.canScrollBackward } }
        Scaffold(
            containerColor = MaterialTheme.colorScheme.background,
            contentWindowInsets = WindowInsets(0),
            snackbarHost = { if (materialNavigation != null) SnackbarHost(snackbar) },
            topBar = {
                materialNavigation?.UpBar(current?.name ?: "Private groups", ::back) {
                    if (id != null) IconButton(enabled = !busy, onClick = { act { open(id) } }) {
                        LineIcon("refresh", "Refresh group", tint = LocalContentColor.current)
                    }
                }
            },
            floatingActionButton = {
                if (materialNavigation != null && current?.phase == "joined" && !current.pending && !busy) {
                    ExtendedFloatingActionButton(onClick = { begin("New post") },
                        modifier = Modifier.semantics { contentDescription = "New post" },
                        expanded = expandedPostAction,
                        icon = { LineIcon("edit", null, tint = LocalContentColor.current) },
                        text = { Text("New post") },
                        containerColor = MaterialTheme.colorScheme.primary,
                        contentColor = MaterialTheme.colorScheme.onPrimary)
                }
            }
        ) { padding ->
            LazyColumn(Modifier.fillMaxSize().padding(padding).consumeWindowInsets(padding).imePadding(),
                state = list,
                contentPadding = PaddingValues(start = 22.dp, end = 22.dp, top = 12.dp,
                    bottom = if (materialNavigation != null && current?.phase == "joined") 88.dp else 24.dp),
                verticalArrangement = Arrangement.spacedBy(14.dp)) {
                if (busy && materialNavigation != null && fields == null) item { GroupLoadingIndicator() }
                sections.filter { it.rows.isNotEmpty() || it.footer.isNotEmpty() }.forEach { section ->
                    item {
                        Column(Modifier.fillMaxWidth(), verticalArrangement = Arrangement.spacedBy(8.dp)) {
                            if (section.title.isNotEmpty()) Text(section.title, style = MaterialTheme.typography.titleMedium)
                            section.rows.forEach { row ->
                                val post = postsById[row.id]
                                when {
                                    post != null -> {
                                        HorizontalDivider(color = MaterialTheme.colorScheme.outlineVariant)
                                        if (post.parent != null && post.parent !in postsById) {
                                            Text("Reply to an earlier post", style = MaterialTheme.typography.labelMedium)
                                        }
                                        Text(post.author, style = MaterialTheme.typography.labelLarge, fontWeight = FontWeight.SemiBold)
                                        post.headline?.let { Text(it, style = postTitleStyle) }
                                        Text(post.body, style = postBodyStyle)
                                    }
                                    row.kind == "field" || row.kind == "multiline" -> PrivateGroupTextField(row)
                                    row.kind == "text" -> {
                                        if (row.id == "empty" && materialNavigation != null) EmptyStateEmblem("groups")
                                        Text(row.title, style = if (row.id == "title") MaterialTheme.typography.headlineSmall
                                            else MaterialTheme.typography.titleMedium)
                                        if (row.detail.isNotEmpty()) Text(row.detail, style = MaterialTheme.typography.bodyMedium)
                                    }
                                    row.id == "create" || row.id == "post" || row.id == "submit" ->
                                        YapButton(row::tap, enabled = row.enabled) {
                                            if (row.id == "create" && materialNavigation != null) ActionIcon("plus")
                                            Text(row.title)
                                        }
                                    row.kind == "disclosure" && materialNavigation != null && row.enabled ->
                                        ProfileOption(row.title, "groups", onClick = row::tap)
                                    row.kind == "disclosure" || row.id == "import" ->
                                        FilledTonalButton(row::tap, enabled = row.enabled) { Text(row.title) }
                                    else -> TextButton(onClick = row::tap, enabled = row.enabled) {
                                        Text(row.title, color = if (row.destructive && row.enabled) MaterialTheme.colorScheme.error
                                            else androidx.compose.ui.graphics.Color.Unspecified)
                                    }
                                }
                            }
                            if (section.footer.isNotEmpty()) Text(section.footer, style = MaterialTheme.typography.bodyMedium,
                                color = MaterialTheme.colorScheme.onSurfaceVariant)
                        }
                    }
                }
            }
        }
        if (fields != null && materialNavigation != null) {
            val model = editorSection(fields)
            val submit = model.rows.last()
            val sheet = rememberEditorSheetState { next ->
                if (next == SheetValue.Hidden && (busy || input.isNotBlank() || heading.isNotBlank())) {
                    if (!busy) discardEditor = true
                    false
                } else true
            }
            EditorSheet(onDismissRequest = ::dismissEditor, sheetState = sheet) {
                Column(Modifier.fillMaxWidth().imePadding()) {
                    Row(Modifier.fillMaxWidth().padding(horizontal = 22.dp), horizontalArrangement = Arrangement.SpaceBetween) {
                        TextButton(enabled = !busy, onClick = ::dismissEditor) { Text("Cancel") }
                        YapButton(enabled = submit.enabled, onClick = submit::tap) { Text(submit.title) }
                    }
                    LazyColumn(contentPadding = PaddingValues(22.dp), verticalArrangement = Arrangement.spacedBy(14.dp)) {
                        item { Text(fields, style = MaterialTheme.typography.headlineMedium) }
                        if (notice.isNotEmpty()) item { Text(notice, color = MaterialTheme.colorScheme.error) }
                        model.rows.dropLast(1).forEach { row ->
                            item {
                                if (row.kind == "text") Text(row.title) else PrivateGroupTextField(row)
                            }
                        }
                        if (model.footer.isNotEmpty()) item {
                            Text(model.footer, color = MaterialTheme.colorScheme.onSurfaceVariant)
                        }
                    }
                }
            }
        }
    }
    if (discardEditor) {
        PlatformConfirmation("Discard changes?", "Your changes will be discarded.", "Discard",
            onCancel = { discardEditor = false }, cancel = "Keep editing") {
            discardEditor = false
            editor = null
            input = ""
            heading = ""
            reply = null
        }
    }
    if (confirmDiscard && id != null) {
        PlatformConfirmation("Discard the pending post?", "The saved draft will be removed from this device.", "Discard draft",
            onCancel = { confirmDiscard = false }) {
            confirmDiscard = false
            act {
                request("discard", "group" to JsonPrimitive(id))
                page = page?.copy(pending = false)
            }
        }
    }
    removeDevice?.let { device ->
        PlatformConfirmation("Remove this device?", "Future posts and replies will use new keys.", "Remove",
            onCancel = { removeDevice = null }) {
            removeDevice = null
            if (id != null) act {
                request("remove", "group" to JsonPrimitive(id), "device" to JsonPrimitive(device))
                open(id)
            }
        }
    }
}


@Composable
private fun PrivateGroupTextField(row: FormRow) {
    OutlinedTextField(row.value, row::edit, enabled = row.enabled, label = { Text(row.title) },
        singleLine = row.kind == "field", minLines = if (row.kind == "multiline") 3 else 1,
        modifier = Modifier.fillMaxWidth())
}
