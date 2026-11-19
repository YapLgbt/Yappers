@file:OptIn(androidx.compose.material3.ExperimentalMaterial3Api::class)

package app.yappers

import androidx.compose.foundation.layout.*
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.selection.selectableGroup
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.dp
import kotlinx.coroutines.CancellationException
import kotlinx.coroutines.launch

/** Public catalog UI. Search and city selection filter the downloaded catalog on-device. */
@Composable
fun GroupsScreen(
    connection: NetkitState,
    onConnect: () -> Unit,
    onCity: (GroupLocality?) -> Unit,
    onOpen: suspend (String) -> Unit,
    onJoin: suspend (GroupSummary) -> Unit,
    onCreate: suspend (String, String, String, GroupLocality?) -> Unit,
    onError: (String) -> Unit,
    onRefresh: (() -> Unit)? = null,
    onHome: suspend () -> Unit = {}
) {
    var tab by remember { mutableStateOf("Joined") }
    var category by remember { mutableStateOf<String?>(null) }
    var query by remember { mutableStateOf("") }
    var choosingCity by remember { mutableStateOf(false) }
    var creating by remember { mutableStateOf(false) }
    var acting by remember { mutableStateOf(false) }
    val scope = rememberCoroutineScope()
    fun act(action: suspend () -> Unit) {
        if (acting) return
        acting = true
        scope.launch {
            try { action() }
            catch (error: Exception) {
                if (error is CancellationException) throw error
                onError(error.message ?: "Couldn't open this group. Please try again.")
            } finally { acting = false }
        }
    }
    fun worldwide() {
        tab = "Worldwide"
        category = null
        query = ""
    }
    val directory = if (tab == "Joined") connection.joinedGroups.filter { group ->
        listOf(group.name, group.description, group.category, group.locality?.label)
            .filterNotNull().any { it.contains(query.trim(), ignoreCase = true) }
    }.sortedBy { it.name.lowercase() }
    else visibleGroups(connection.groups, tab == "Worldwide", connection.locality, query)
    val categories = directory.map { it.category }.distinct().sorted()
    val showCategories = tab != "Joined" && directory.size >= 3 && categories.size > 1
    val groups = directory.filter { !showCategories || category !in categories || it.category == category }
    val scopes = listOf("Joined", "Local", "Worldwide")
    val canSwitch = !acting && connection.pendingCount == 0
    val status = if (connection.enabled && !connection.groupsBusy) connection.groupsStatus.ifBlank {
        if (!connection.online && directory.isNotEmpty()) "Showing saved groups. Reconnect to update the directory." else ""
    } else ""
    val emptyTitle = when {
        query.isNotBlank() -> "No matching groups"
        tab == "Joined" -> "No joined groups yet"
        else -> "No groups here yet"
    }
    val showWorldwide = query.isBlank() && connection.enabled && ((tab == "Local" && groups.size < 3) ||
        (tab == "Joined" && groups.isEmpty()))
    fun selectScope(value: String) { tab = value; category = null }
    fun search(value: String) { query = value.take(120); category = null }
    fun canOpen(group: GroupSummary, joined: Boolean) = !acting && (joined || connection.enabled) &&
        (connection.pendingCount == 0 || connection.activeGroup?.id == group.id)
    fun open(group: GroupSummary, joined: Boolean) {
        if (canOpen(group, joined)) act { if (joined) onOpen(group.id) else onJoin(group) }
    }

    val native = LocalNativeForm.current
    if (native != null) {
        val sections = buildList {
            add(FormSection(title = "Discover groups", rows = buildList {
                if (connection.enabled) {
                    add(FormRow("create", "Create group", enabled = canSwitch, symbol = "plus", onTap = { creating = true }))
                    if (connection.hasHomeFeed && connection.activeGroup != null) {
                        add(FormRow("home", "Home feed", enabled = canSwitch, onTap = { act { onHome() } }))
                    }
                } else add(FormRow("connect", "Community settings", kind = "disclosure",
                    detail = "Join a community to browse groups.", onTap = onConnect))
            }, footer = if (connection.pendingCount > 0) "Your posts are still sending. You can switch groups when they finish." else ""))
            add(FormSection(rows = buildList {
                add(FormRow("scope", "Group scope", "segments", value = tab,
                    choices = scopes.map { value -> FormChoice(value, selected = value == tab) { selectScope(value) } }))
                if (tab == "Local") add(FormRow("city", connection.locality?.label ?: "Choose a city", kind = "disclosure",
                    onTap = { choosingCity = true }))
                add(FormRow("search", "Search groups", "search", value = query, limit = 120, onChange = ::search))
                if (showCategories) add(FormRow("category", "Category", "menu", value = category?.takeIf { it in categories } ?: "All categories",
                    choices = listOf(FormChoice("All categories", category !in categories) { category = null }) +
                        categories.map { value -> FormChoice(value, value == category) { category = value } }))
            }, footer = if (connection.groupsBusy) "Refreshing groups…" else status))
            val results = buildList {
                groups.forEach { group ->
                    val joined = connection.joinedGroups.any { it.id == group.id }
                    val label = "${if (joined) "Open" else "Join"} group ${group.name}"
                    add(FormRow("group-${group.id}", label, kind = "disclosure", enabled = canOpen(group, joined),
                        detail = "${group.description}\n${group.locality?.label ?: "Worldwide"} · ${group.category}",
                        identifier = label, onTap = { open(group, joined) }))
                }
                if (groups.isEmpty() && !connection.groupsBusy) {
                    add(FormRow("empty", emptyTitle, kind = "text"))
                    if (tab == "Joined" && query.isBlank() && connection.enabled) {
                        add(FormRow("browse-local", "Browse local groups", onTap = { selectScope("Local") }))
                    }
                }
                if (showWorldwide) add(FormRow("browse-worldwide", "Browse worldwide groups", onTap = ::worldwide))
            }
            if (results.isNotEmpty()) add(FormSection(rows = results))
        }
        native(NativeFormModel(sections = sections, onRefresh = onRefresh), "screen", Modifier.fillMaxSize())
    } else LazyColumn(Modifier.fillMaxSize(), contentPadding = PaddingValues(bottom = 24.dp)) {
        item {
            Column(Modifier.fillMaxWidth().padding(horizontal = 22.dp, vertical = 12.dp),
                verticalArrangement = Arrangement.spacedBy(8.dp)) {
                Text("Discover groups", style = MaterialTheme.typography.headlineMedium, fontWeight = FontWeight.Bold)
                if (connection.enabled) FlowRow(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                    Button(enabled = canSwitch, onClick = { creating = true }) { Text("Create group") }
                    if (connection.hasHomeFeed && connection.activeGroup != null) TextButton(enabled = canSwitch,
                        onClick = { act { onHome() } }) { Text("Home feed") }
                } else {
                    Text("Join a community to browse groups.", style = MaterialTheme.typography.bodyMedium)
                    Button(onClick = onConnect) { Text("Community settings") }
                }
                if (connection.pendingCount > 0) Text("Your posts are still sending. You can switch groups when they finish.",
                    style = MaterialTheme.typography.bodyMedium)
                BoxWithConstraints(Modifier.fillMaxWidth()) {
                    if (maxWidth >= 324.dp * LocalDensity.current.fontScale) {
                        SingleChoiceSegmentedButtonRow(Modifier.fillMaxWidth()) {
                            scopes.forEachIndexed { index, label ->
                                SegmentedButton(selected = tab == label, onClick = { selectScope(label) },
                                    shape = SegmentedButtonDefaults.itemShape(index, scopes.size)) { Text(label) }
                            }
                        }
                    } else {
                        FlowRow(Modifier.selectableGroup(), horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                            scopes.forEach { label ->
                                FilterChip(selected = tab == label, onClick = { selectScope(label) },
                                    label = { Text(label) })
                            }
                        }
                    }
                }
                if (tab == "Local") OutlinedButton(onClick = { choosingCity = true }) {
                    Text(connection.locality?.label ?: "Choose a city")
                }
                OutlinedTextField(value = query, onValueChange = ::search, singleLine = true,
                    label = { Text("Search groups") }, modifier = Modifier.fillMaxWidth())
                if (showCategories) FlowRow(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                    FilterChip(selected = category == null || category !in categories, onClick = { category = null },
                        label = { Text("All categories") })
                    categories.forEach { value ->
                        FilterChip(selected = category == value, onClick = { category = if (category == value) null else value },
                            label = { Text(value) })
                    }
                }
                if (status.isNotBlank()) Text(status, style = MaterialTheme.typography.bodySmall,
                    color = MaterialTheme.colorScheme.onSurfaceVariant)
                if (connection.groupsBusy) LinearProgressIndicator(Modifier.fillMaxWidth())
            }
        }
        items(groups, key = { it.id }) { group ->
            val joined = connection.joinedGroups.any { it.id == group.id }
            Row(Modifier.fillMaxWidth().padding(horizontal = 22.dp, vertical = 12.dp),
                verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(12.dp)) {
                Column(Modifier.weight(1f), verticalArrangement = Arrangement.spacedBy(4.dp)) {
                    Text(group.name, style = MaterialTheme.typography.titleMedium, fontWeight = FontWeight.Bold)
                    Text(group.description, style = MaterialTheme.typography.bodyMedium)
                    Text("${group.locality?.label ?: "Worldwide"} · ${group.category}",
                        color = MaterialTheme.colorScheme.onSurfaceVariant, style = MaterialTheme.typography.bodySmall)
                }
                FilledTonalButton(enabled = canOpen(group, joined),
                    modifier = Modifier.semantics { contentDescription = "${if (joined) "Open" else "Join"} group ${group.name}" },
                    onClick = { open(group, joined) }) { Text(if (joined) "Open" else "Join") }
            }
            HorizontalDivider(Modifier.padding(horizontal = 22.dp), color = MaterialTheme.colorScheme.outlineVariant)
        }
        if (groups.isEmpty() && !connection.groupsBusy) item {
            Column(Modifier.padding(horizontal = 22.dp, vertical = 12.dp), verticalArrangement = Arrangement.spacedBy(4.dp)) {
                Text(emptyTitle, style = MaterialTheme.typography.titleMedium)
                if (tab == "Joined" && query.isBlank() && connection.enabled) TextButton(onClick = { selectScope("Local") }) {
                    Text("Browse local groups")
                }
            }
        }
        if (showWorldwide) item {
            TextButton(modifier = Modifier.padding(horizontal = 16.dp), onClick = ::worldwide) { Text("Browse worldwide groups") }
        }
    }
    if (choosingCity) CitySheet(connection.groups, connection.locality,
        onDismiss = { choosingCity = false }, onSelect = { onCity(it); choosingCity = false })
    if (creating) CreateGroupSheet(connection.locality, onDismiss = { creating = false }) { name, description, category, locality ->
        onCreate(name, description, category, locality)
        creating = false
    }
}

@Composable
private fun CitySheet(groups: List<GroupSummary>, selected: GroupLocality?, onDismiss: () -> Unit, onSelect: (GroupLocality?) -> Unit) {
    var query by remember { mutableStateOf("") }
    val cities = groups.mapNotNull { it.locality }.fold(emptyList<GroupLocality>()) { all, city ->
        if (all.any { it.samePlace(city) }) all else all + city
    }.sortedBy { it.label }
    val matches = cities.filter { it.label.contains(query.trim(), ignoreCase = true) }
    val noMatch = if (cities.isEmpty()) "New cities appear when someone creates a local group." else "No cities match your search."
    val native = LocalNativeForm.current
    if (native != null) {
        val rows = buildList {
            add(FormRow("find", "Find a city", "search", value = query, limit = 100, onChange = { query = it.take(100) }))
            add(FormRow("all", "Explore all cities", symbol = if (selected == null) "checkmark" else "", onTap = { onSelect(null) }))
            matches.forEach { city ->
                add(FormRow("city-${city.label}", city.label, symbol = if (selected?.samePlace(city) == true) "checkmark" else "",
                    onTap = { onSelect(city) }))
            }
            if (matches.isEmpty()) add(FormRow("empty", noMatch, kind = "text"))
        }
        native(NativeFormModel("Choose a city", listOf(FormSection(rows = rows)), leading = FormChoice("Done", action = onDismiss),
            onDismiss = onDismiss), "sheet", Modifier)
        return
    }
    ModalBottomSheet(onDismissRequest = onDismiss, sheetState = rememberModalBottomSheetState(skipPartiallyExpanded = true)) {
        LazyColumn(Modifier.fillMaxWidth().imePadding(), contentPadding = PaddingValues(22.dp), verticalArrangement = Arrangement.spacedBy(12.dp)) {
            item { Text("Choose a city", style = MaterialTheme.typography.headlineMedium, fontWeight = FontWeight.Bold) }
            item { OutlinedTextField(query, { query = it.take(100) }, label = { Text("Find a city") }, modifier = Modifier.fillMaxWidth(), singleLine = true) }
            item { TextButton(onClick = { onSelect(null) }) { Text("Explore all cities") } }
            items(matches) { city ->
                OutlinedButton(onClick = { onSelect(city) }, modifier = Modifier.fillMaxWidth()) { Text(city.label, fontWeight = if (selected?.samePlace(city) == true) FontWeight.Bold else FontWeight.Normal) }
            }
            if (matches.isEmpty()) item { Text(noMatch) }
            item { TextButton(onClick = onDismiss) { Text("Done") } }
        }
    }
}

@Composable
private fun CreateGroupSheet(initialCity: GroupLocality?, onDismiss: () -> Unit,
    onCreate: suspend (String, String, String, GroupLocality?) -> Unit) {
    var name by remember { mutableStateOf("") }
    var description by remember { mutableStateOf("") }
    var category by remember { mutableStateOf("Friends") }
    var worldwide by remember { mutableStateOf(false) }
    var city by remember { mutableStateOf(initialCity?.city ?: "") }
    var region by remember { mutableStateOf(initialCity?.region ?: "") }
    var country by remember { mutableStateOf(initialCity?.country ?: "") }
    var saving by remember { mutableStateOf(false) }
    var discard by remember { mutableStateOf(false) }
    val dirty = name.isNotEmpty() || description.isNotEmpty() || category != "Friends" || worldwide ||
        city != initialCity?.city.orEmpty() || region != initialCity?.region.orEmpty() || country != initialCity?.country.orEmpty()
    fun dismissDraft() {
        if (saving) return
        if (dirty) discard = true else onDismiss()
    }
    var error by remember { mutableStateOf<String?>(null) }
    val scope = rememberCoroutineScope()
    val valid = name.trim().isNotEmpty() && description.trim().isNotEmpty() &&
        (worldwide || (city.trim().isNotEmpty() && country.length == 2 && country.all { it in 'A'..'Z' }))
    fun save() {
        if (!valid || saving) return
        saving = true
        error = null
        scope.launch {
            try { onCreate(name, description, category, if (worldwide) null else GroupLocality(city.trim(), region.trim(), country)) }
            catch (failure: Exception) {
                if (failure is CancellationException) throw failure
                error = failure.message ?: "Couldn't confirm creation. Retry with the same details."
            } finally { saving = false }
        }
    }
    fun changeCountry(text: String) { country = text.filter(Char::isLetter).take(2).uppercase() }
    val visibility = "Anyone in a participating community can find this group and read its posts."
    val discardMessage = "Your group hasn't been created. Your changes will be discarded."
    val native = LocalNativeForm.current
    if (native != null) {
        val sections = buildList {
            add(FormSection(rows = listOf(
                FormRow("name", "Group name", "field", value = name, enabled = !saving, limit = 60,
                    onChange = { if (it.length <= 60) name = it }),
                FormRow("description", "What brings you together?", "multiline", value = description, enabled = !saving,
                    limit = 300, onChange = { if (it.length <= 300) description = it }),
                FormRow("category", "A group for…", "menu", value = category, enabled = !saving, choices = topics.drop(1).map {
                    FormChoice(it, selected = it == category) { category = it }
                })
            ), footer = visibility))
            add(FormSection(rows = buildList {
                add(FormRow("worldwide", "Worldwide group", "toggle", checked = worldwide, enabled = !saving, onToggle = { worldwide = it }))
                if (!worldwide) {
                    add(FormRow("city", "City", "field", value = city, enabled = !saving, limit = 60, onChange = { if (it.length <= 60) city = it }))
                    add(FormRow("region", "Region (optional)", "field", value = region, enabled = !saving, limit = 60, onChange = { if (it.length <= 60) region = it }))
                    add(FormRow("country", "Country code (US, GB…)", "field", value = country, enabled = !saving, limit = 2,
                        keyboard = "asciiUpper", onChange = ::changeCountry))
                }
            }, footer = if (worldwide) "" else "People from any city can join."))
            error?.let { add(FormSection(rows = listOf(FormRow("error", it, kind = "text")))) }
        }
        val confirmation = if (discard) FormConfirmation("Discard group draft?", discardMessage,
            FormChoice("Discard draft", destructive = true) { discard = false; onDismiss() },
            FormChoice("Keep editing") { discard = false }) else null
        native(NativeFormModel("New group", sections, leading = FormChoice("Cancel", action = ::dismissDraft), leadingEnabled = !saving,
            trailing = FormChoice(if (saving) "Creating…" else "Create group", action = ::save), trailingEnabled = valid && !saving,
            canDismiss = !saving && !dirty, confirmation = confirmation, onDismiss = ::dismissDraft), "sheet", Modifier)
        return
    }
    val sheet = rememberEditorSheetState(confirmValueChange = { next ->
        if (next == SheetValue.Hidden && (saving || dirty)) {
            if (!saving) discard = true
            false
        } else true
    })
    EditorSheet(onDismissRequest = { dismissDraft() }, sheetState = sheet) {
        Column(Modifier.fillMaxWidth().imePadding()) {
            Row(Modifier.fillMaxWidth().padding(horizontal = 22.dp), horizontalArrangement = Arrangement.SpaceBetween) {
                TextButton(enabled = !saving, onClick = { dismissDraft() }) { Text("Cancel") }
                Button(enabled = valid && !saving, onClick = ::save) { Text(if (saving) "Creating…" else "Create group") }
            }
            LazyColumn(contentPadding = PaddingValues(22.dp), verticalArrangement = Arrangement.spacedBy(14.dp)) {
                item { Text("New group", style = MaterialTheme.typography.headlineMedium, fontWeight = FontWeight.Bold) }
                item { Text(visibility, color = MaterialTheme.colorScheme.onSurfaceVariant) }
                error?.let { message -> item { Text(message, color = MaterialTheme.colorScheme.error) } }
                item { OutlinedTextField(name, { if (it.length <= 60) name = it }, enabled = !saving, label = { Text("Group name") }, singleLine = true, modifier = Modifier.fillMaxWidth()) }
                item { OutlinedTextField(description, { if (it.length <= 300) description = it }, enabled = !saving, label = { Text("What brings you together?") }, minLines = 3, modifier = Modifier.fillMaxWidth()) }
                item { Text("A group for…", style = MaterialTheme.typography.titleMedium) }
                item { FlowRow(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                    topics.drop(1).forEach { value -> FilterChip(enabled = !saving, selected = category == value, onClick = { category = value }, label = { Text(value) }) }
                } }
                item { Row(Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.SpaceBetween) {
                    Text("Worldwide group", Modifier.weight(1f).padding(top = 12.dp))
                    Switch(worldwide, { worldwide = it }, enabled = !saving, modifier = Modifier.semantics { contentDescription = "Worldwide group" })
                } }
                if (!worldwide) {
                    item { OutlinedTextField(city, { if (it.length <= 60) city = it }, enabled = !saving, label = { Text("City") }, singleLine = true, modifier = Modifier.fillMaxWidth()) }
                    item { OutlinedTextField(region, { if (it.length <= 60) region = it }, enabled = !saving, label = { Text("Region (optional)") }, singleLine = true, modifier = Modifier.fillMaxWidth()) }
                    item { OutlinedTextField(country, ::changeCountry, enabled = !saving, label = { Text("Country code (US, GB…)") }, singleLine = true, modifier = Modifier.fillMaxWidth()) }
                    item { Text("People from any city can join.", style = MaterialTheme.typography.bodySmall, color = MaterialTheme.colorScheme.onSurfaceVariant) }
                }
            }
        }
    }
    if (discard) AlertDialog(onDismissRequest = { discard = false }, title = { Text("Discard group draft?") },
        text = { Text(discardMessage) },
        confirmButton = { TextButton(onClick = { discard = false; onDismiss() }) { Text("Discard draft") } },
        dismissButton = { TextButton(onClick = { discard = false }) { Text("Keep editing") } })

}
