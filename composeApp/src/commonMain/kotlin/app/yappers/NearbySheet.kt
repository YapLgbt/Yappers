@file:OptIn(androidx.compose.material3.ExperimentalMaterial3Api::class)
package app.yappers

import androidx.compose.foundation.layout.*
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.ui.Modifier
import androidx.compose.ui.text.input.KeyboardType
import androidx.compose.ui.unit.dp
import kotlinx.coroutines.CancellationException
import kotlinx.coroutines.delay
import kotlinx.coroutines.launch
import kotlinx.serialization.json.Json

internal interface OfflineTownCatalog

@Composable
internal fun NearbySheet(initial: NearbyPreference, services: NearbyServices?, onDismiss: () -> Unit,
                         onApply: (NearbyPreference) -> Unit) {
    var place by remember { mutableStateOf(initial.center) }
    var radius by remember { mutableStateOf(initial.radiusMiles.toString()) }
    var query by remember { mutableStateOf("") }
    var matches by remember { mutableStateOf(emptyList<PostArea>()) }
    var status by remember { mutableStateOf("") }
    var locating by remember { mutableStateOf(false) }
    val scope = rememberCoroutineScope()
    LaunchedEffect(query) {
        matches = emptyList()
        if (query.trim().length < 2 || services == null) return@LaunchedEffect
        delay(350)
        status = "Searching…"
        try {
            matches = Json.decodeFromString<List<PostArea>>(services.perform("search", query.trim())).distinct()
            status = if (matches.isEmpty()) "No places found. Try adding a state or country." else ""
        } catch (failure: Exception) {
            if (failure is CancellationException) throw failure
            status = failure.message.orEmpty()
        }
    }
    val canApply = !locating && (place == null || radius.toIntOrNull() in 1..250)
    fun apply() {
        if (canApply) onApply(NearbyPreference(place, radius.toIntOrNull()?.takeIf { it in 1..250 } ?: initial.radiusMiles))
    }
    fun select(area: PostArea?) {
        place = area
        query = ""
        status = ""
        matches = emptyList()
    }
    fun search(text: String) { query = text.take(120); status = ""; matches = emptyList() }
    fun changeRadius(text: String) { if (text.length <= 3 && text.all(Char::isDigit)) radius = text }
    fun locate() {
        if (services == null || locating) return
        locating = true
        query = ""
        matches = emptyList()
        status = "Finding your area…"
        scope.launch {
            try { select(Json.decodeFromString<PostArea>(services.perform("locate"))) }
            catch (failure: Exception) {
                if (failure is CancellationException) throw failure
                status = failure.message.orEmpty()
            } finally { locating = false }
        }
    }
    val guidance = "Choose where to browse and post. Posts share your approximate area, even when anonymous. Choose All cities to post without a location."
    val locationTitle = if (locating) "Locating…" else "Use my current location"
    val native = LocalNativeForm.current
    if (native != null) {
        val sections = buildList {
            add(FormSection(rows = listOf(
                FormRow("place", place?.label ?: "All cities", kind = "text"),
                FormRow("locate", locationTitle, enabled = services != null && !locating, symbol = "location", onTap = ::locate)
            ), footer = guidance))
            add(FormSection(rows = buildList {
                add(FormRow("search", "Search cities and towns", "search", value = query, limit = 120,
                    enabled = !locating, onChange = ::search))
                if (status.isNotEmpty()) add(FormRow("status", status, kind = "text"))
                matches.forEach { area ->
                    add(FormRow("place-${area.label}-${area.lat100}-${area.lon100}", area.label, onTap = { select(area) }))
                }
            }, footer = if (services is OfflineTownCatalog) "Town data © GeoNames · CC BY 4.0" else ""))
            if (place != null) add(FormSection(rows = listOf(
                FormRow("radius", "Radius in miles", "field", value = radius, keyboard = "number", limit = 3,
                    onChange = ::changeRadius),
                FormRow("all", "Browse all cities", enabled = !locating, onTap = { select(null) })
            ), footer = "Choose 1–250 miles"))
        }
        native(NativeFormModel("Your nearby feed", sections, leading = FormChoice("Cancel", action = onDismiss),
            trailing = FormChoice("Apply", action = ::apply), trailingEnabled = canApply, onDismiss = onDismiss),
            "sheet", Modifier)
        return
    }
    EditorSheet(onDismiss, rememberModalBottomSheetState(skipPartiallyExpanded = true)) {
        Row(Modifier.fillMaxWidth().padding(horizontal = 20.dp, vertical = 8.dp), horizontalArrangement = Arrangement.SpaceBetween) {
            TextButton(onClick = onDismiss) { Text("Cancel") }
            Button(enabled = canApply, onClick = ::apply) { Text("Apply") }
        }
        LazyColumn(Modifier.fillMaxWidth().imePadding(), contentPadding = PaddingValues(22.dp), verticalArrangement = Arrangement.spacedBy(12.dp)) {
            item { Text("Your nearby feed", style = MaterialTheme.typography.headlineMedium) }
            item { Text(guidance) }
            item { Text(place?.label ?: "All cities", style = MaterialTheme.typography.titleLarge) }
            item {
                OutlinedButton(enabled = services != null && !locating, onClick = ::locate) { Text(locationTitle) }
            }
            item { OutlinedTextField(query, ::search, enabled = !locating,
                modifier = Modifier.fillMaxWidth(), label = { Text("Search cities and towns") }, singleLine = true) }
            if (status.isNotEmpty()) item { Text(status, color = MaterialTheme.colorScheme.onSurfaceVariant) }
            items(matches) { result -> TextButton(onClick = { select(result) }) { Text(result.label) } }
            if (services is OfflineTownCatalog) item { Text("Town data © GeoNames · CC BY 4.0", style = MaterialTheme.typography.bodySmall) }
            if (place != null) {
                item { OutlinedTextField(radius, ::changeRadius,
                    label = { Text("Radius in miles") }, supportingText = { Text("Choose 1–250 miles") },
                    modifier = Modifier.fillMaxWidth(), singleLine = true, keyboardOptions = KeyboardOptions(keyboardType = KeyboardType.Number)) }
                item { TextButton(enabled = !locating, onClick = { select(null) }) { Text("Browse all cities") } }
            }
        }
    }
}
