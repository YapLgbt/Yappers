@file:OptIn(androidx.compose.material3.ExperimentalMaterial3Api::class)

package app.yappers

import androidx.compose.foundation.text.KeyboardActions
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.platform.LocalFocusManager
import androidx.compose.ui.text.input.ImeAction
import androidx.compose.foundation.Image
import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.ui.semantics.heading
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.semantics.stateDescription
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.layout.ContentScale
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.Dp
import androidx.compose.ui.unit.dp
import app.yappers.resources.Res
import app.yappers.resources.default_avatar
import kotlinx.coroutines.CancellationException
import kotlinx.coroutines.launch
import kotlin.io.encoding.Base64
import org.jetbrains.compose.resources.painterResource

@Composable
internal fun ProfileAvatar(name: String, profile: CommunityProfile?, size: Dp) {
    val image = remember(profile?.photo) { profile?.photo?.takeIf { it.isNotEmpty() }?.let(::decodeProfilePhoto) }
    Box(Modifier.size(size).clip(CircleShape).background(MaterialTheme.colorScheme.primaryContainer), contentAlignment = Alignment.Center) {
        if (image != null) Image(image, "Profile photo of $name", Modifier.fillMaxSize(), contentScale = ContentScale.Crop)
        else if (profile?.photo == "") Image(painterResource(Res.drawable.default_avatar), null, Modifier.fillMaxSize())
        else LineIcon("person", null)
    }
}

/** A focused editor: explicit Save, bounded free-form fields, and no mandatory identity labels. */
@Composable
internal fun ProfileEditor(initialName: String, initial: CommunityProfile, library: ProfilePhotoLibrary?,
                           creating: Boolean = false, onDismiss: () -> Unit, onSave: suspend (String, CommunityProfile) -> Unit) {
    var name by remember { mutableStateOf(initialName) }
    var profile by remember { mutableStateOf(initial) }
    var saving by remember { mutableStateOf(false) }
    val nativeForm = LocalNativeForm.current
    var cropImage by remember { mutableStateOf<CropSource?>(null) }
    var cropPreview by remember { mutableStateOf("") }
    DisposableEffect(cropImage) { val image = cropImage; onDispose { image?.close() } }
    var choosingPhoto by remember { mutableStateOf(false) }
    var error by remember { mutableStateOf<String?>(null) }
    var discard by remember { mutableStateOf(false) }
    val dirty = name != initialName || profile != initial
    val scope = rememberCoroutineScope()
    fun close() {
        if (saving || choosingPhoto) return
        if (dirty) discard = true else onDismiss()
    }
    val displayName = name.trim()
    val canSave = (creating || dirty) && displayName.length in 2..30 && !saving && !choosingPhoto
    fun save() {
        if (!canSave) return
        saving = true
        scope.launch {
            try { onSave(displayName, profile.cleaned()); if (!creating) onDismiss() }
            catch (failure: Exception) {
                if (failure is CancellationException) throw failure
                error = failure.message ?: "Couldn't save your profile. Try again."
            } finally { saving = false }
        }
    }
    fun choosePhoto() {
        if (library == null || choosingPhoto || saving) return
        choosingPhoto = true
        library.choosePhoto { photo, failure ->
            choosingPhoto = false
            if (photo != null) runCatching { decodeCropSource(Base64.Default.decode(photo)) }
                .onSuccess { cropImage = it; if (nativeForm != null) cropPreview = photo }
                .onFailure { error = "Couldn’t open this photo. Try another image." }
            if (failure != null) error = failure
        }
    }
    fun intention(value: String) {
        profile = profile.copy(hereFor = if (value in profile.hereFor) profile.hereFor - value else profile.hereFor + value)
    }
    AppBackHandler(true) { if (cropImage != null) cropImage = null else close() }
    if (nativeForm != null) {
        val active = !saving && !choosingPhoto
        fun field(title: String, value: () -> String, limit: Int, edit: (String) -> Unit, multiline: Boolean = false): FormRow {
            val length = value().length
            val counter = if (length >= limit - minOf(100, limit / 5)) "$length / $limit" else ""
            return FormRow(title, title, if (multiline) "multiline" else "field", detail = counter,
                fieldValue = value, limit = limit, enabled = active, onChange = { if (active && it.length <= limit) edit(it) })
        }
        val dialogs = if (discard) FormConfirmation("Discard profile changes?", "Your saved profile will stay as it is.",
            FormChoice("Discard", destructive = true, action = onDismiss), FormChoice("Keep editing") { discard = false })
            else error?.let { message -> FormConfirmation("Couldn't update your profile", message,
                FormChoice("OK") { error = null }, FormChoice("") {}) }
        val basics = listOf(
            field("Display name", { name }, 30, { name = it }),
            FormRow("suggest-name", if (creating && name.isBlank()) "Suggest a name" else "Suggest another name", enabled = active, onTap = { name = suggestName(name) }),
            field("Pronouns", { profile.pronouns }, 40, { profile = profile.copy(pronouns = it) }),
        )
        val about = listOf(
            field("Bio", { profile.bio }, 280, { profile = profile.copy(bio = it) }, true),
            field("Identity, in your words", { profile.identity }, 80, { profile = profile.copy(identity = it) }),
            field("Current joys", { profile.interests }, 120, { profile = profile.copy(interests = it) }, true),
        )
        val intentions = profileIntentions.map { value ->
            FormRow(value, value, "choice", checked = value in profile.hereFor, enabled = active, onTap = { intention(value) })
        }
        val photoAction = FormRow("pick-photo", if (profile.photo.isBlank()) "Add photo" else "Change photo", "secondary",
            symbol = "photo", enabled = active && library != null, onTap = ::choosePhoto)
        val control = LocalFeedControl.current
        if (creating && control != null) {
            var section by remember { mutableStateOf<String?>(null) }
            EstuaryOnboardingSurface(header = {
                control(FormRow("back", "Back", "back", symbol = "chevron.backward", enabled = active, onTap = ::close), Modifier)
            }, footer = {
                control(FormRow("continue", if (saving) "Creating account…" else "Continue to username", "primary",
                    enabled = canSave, onTap = ::save), Modifier.fillMaxWidth().padding(horizontal = 12.dp))
            }) {
                Text("Create your profile", style = MaterialTheme.typography.headlineLarge, fontWeight = FontWeight.Bold)
                Text("Choose what neighbors see on posts you make by name.", style = MaterialTheme.typography.bodyLarge)
                Row(verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(16.dp)) {
                    ProfileAvatar(displayName, profile, 80.dp)
                    Column(Modifier.weight(1f), verticalArrangement = Arrangement.spacedBy(4.dp)) {
                        Text(displayName.ifEmpty { "Your display name" }, style = MaterialTheme.typography.titleLarge, fontWeight = FontWeight.SemiBold)
                        if (profile.pronouns.isNotBlank()) Text(profile.pronouns, style = MaterialTheme.typography.bodyMedium)
                        control(photoAction, Modifier)
                        if (profile.photo.isNotBlank()) control(FormRow("remove-photo", "Remove photo", "back", destructive = true,
                            enabled = active, onTap = { profile = profile.copy(photo = "") }), Modifier)
                    }
                }
                Column {
                    control(FormRow("basics", "Name and pronouns", "method", detail = "Set your display name and pronouns.",
                        symbol = "person.text.rectangle", enabled = active) { section = "Name and pronouns" }, Modifier.fillMaxWidth())
                    HorizontalDivider(color = MaterialTheme.colorScheme.outlineVariant)
                    control(FormRow("about", "About you", "method", detail = "Bio, identity and current joys.",
                        symbol = "text.alignleft", enabled = active) { section = "About you" }, Modifier.fillMaxWidth())
                    HorizontalDivider(color = MaterialTheme.colorScheme.outlineVariant)
                    control(FormRow("here-for", "Here for", "method", detail = profile.hereFor.joinToString(", ").ifEmpty { "Choose what brings you here." },
                        symbol = "person.2", enabled = active) { section = "Here for" }, Modifier.fillMaxWidth())
                }
                if (displayName.length !in 2..30) Text("Choose a display name of at least two characters to continue.")
                error?.let { Text(it, color = MaterialTheme.colorScheme.error) }
            }
            section?.let { title ->
                val rows = when (title) { "Name and pronouns" -> basics; "About you" -> about; else -> intentions }
                val done = { section = null }
                nativeForm(NativeFormModel(title, listOf(FormSection(rows = rows)), trailing = FormChoice("Done", action = done), onDismiss = done),
                    if (title == "About you") "sheet" else "compactSheet", Modifier)
            }
            if (discard) PlatformConfirmation("Discard your profile?", "Your draft will be cleared.", "Discard",
                onCancel = { discard = false }, cancel = "Keep editing", onConfirm = onDismiss)
        } else {
            val sections = listOf(
                FormSection("Photo", buildList {
                    add(FormRow("photo-preview", "Profile photo", "photo", value = profile.photo))
                    add(FormRow("pick-photo", photoAction.title, symbol = "photo", enabled = photoAction.enabled, onTap = ::choosePhoto))
                    if (profile.photo.isNotBlank()) add(FormRow("remove-photo", "Remove photo", destructive = true, enabled = active,
                        onTap = { profile = profile.copy(photo = "") }))
                }),
                FormSection(rows = basics + about),
                FormSection("Here for", intentions, footer = "These details appear on new posts you make by name.")
            )
            nativeForm(NativeFormModel("Edit profile", sections, leading = FormChoice("Cancel", action = ::close),
                trailing = FormChoice(if (saving) "Saving…" else "Save", action = ::save), trailingEnabled = canSave, leadingEnabled = active,
                canDismiss = !dirty && active, confirmation = dialogs, onDismiss = ::close), "sheet", Modifier)
        }
        val image = cropImage
        val crop = LocalNativeCrop.current
        if (image != null && crop != null) {
            fun finish() { cropImage = null; cropPreview = "" }
            val model = remember(image) { NativePhotoCrop(cropPreview, image, ::finish) { photo ->
                profile = profile.copy(photo = photo)
                finish()
            } }
            crop(model)
        }
        return
    }
    val source = cropImage
    if (source != null) PhotoCropEditor(source, onCancel = { cropImage = null }, onCrop = { photo ->
        profile = profile.copy(photo = photo)
        cropImage = null
    }) else Column(Modifier.widthIn(max = 760.dp).fillMaxWidth().fillMaxHeight().imePadding().padding(top = 12.dp)) {
        Row(Modifier.fillMaxWidth().padding(horizontal = 22.dp), verticalAlignment = Alignment.CenterVertically) {
            YapCancelButton(enabled = !saving && !choosingPhoto, onClick = { close() })
            Spacer(Modifier.weight(1f))
            YapButton(enabled = canSave, onClick = ::save) { Text(if (saving) "Saving…" else "Save profile") }
        }
        LazyColumn(Modifier.weight(1f), contentPadding = PaddingValues(22.dp), verticalArrangement = Arrangement.spacedBy(16.dp)) {
            item { Text("Edit your profile", style = MaterialTheme.typography.headlineMedium, fontWeight = FontWeight.Bold) }
            item { Row(verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(16.dp)) {
                ProfileAvatar(name, profile, 80.dp)
                Column {
                    OutlinedButton(enabled = library != null && !choosingPhoto && !saving, onClick = ::choosePhoto) { Text(if (choosingPhoto) "Choosing…" else if (profile.photo.isBlank()) "Add photo" else "Change photo") }
                    if (profile.photo.isNotBlank()) TextButton(onClick = { profile = profile.copy(photo = "") }) { Text("Remove photo") }
                }
            } }
            item { Column {
                ProfileField("Display name", name, 30, singleLine = true) { name = it }
                TextButton(enabled = !saving && !choosingPhoto, onClick = { name = suggestName(name) }) { Text("Suggest another name") }
            } }
            item { ProfileField("Pronouns", profile.pronouns, 40, "she / they, he / him, ask me…") { profile = profile.copy(pronouns = it) } }
            item { ProfileField("Bio", profile.bio, 280, "A little about your world", lines = 3) { profile = profile.copy(bio = it) } }
            item { ProfileField("Identity, in your words", profile.identity, 80, "Queer, trans, questioning, wonderfully you…") { profile = profile.copy(identity = it) } }
            item { Text("Here for", style = MaterialTheme.typography.titleMedium, fontWeight = FontWeight.SemiBold)
                FlowRow(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                    profileIntentions.forEach { value -> YapFilterChip(selected = value in profile.hereFor,
                        onClick = { intention(value) }, label = { Text(value) }) }
                }
            }
            item { ProfileField("Current joys", profile.interests, 120, "Your tiny obsessions, hobbies, special interests…") { profile = profile.copy(interests = it) } }
            item { Text("These details appear on new posts you make by name.",
                style = MaterialTheme.typography.bodySmall, color = MaterialTheme.colorScheme.onSurfaceVariant) }
        }
    }
    error?.let { message -> AlertDialog(onDismissRequest = { error = null }, title = { Text("Couldn't update your profile") },
        text = { Text(message) }, confirmButton = { TextButton(onClick = { error = null }) { Text("OK") } }) }
    if (discard) AlertDialog(onDismissRequest = { discard = false }, title = { Text("Discard profile changes?") },
        text = { Text("Your saved profile will stay as it is.") },
        confirmButton = { TextButton(onClick = onDismiss) { Text("Discard") } },
        dismissButton = { TextButton(onClick = { discard = false }) { Text("Keep editing") } })
}

@Composable
private fun ProfileField(label: String, value: String, limit: Int, placeholder: String = "", lines: Int = 1,
                         singleLine: Boolean = false, onChange: (String) -> Unit) {
    val focus = LocalFocusManager.current
    OutlinedTextField(value = value, onValueChange = { if (it.length <= limit) onChange(it) },
        label = { Text(label) }, placeholder = { Text(placeholder) }, supportingText = { Text("${value.length} / $limit") },
        keyboardOptions = KeyboardOptions(imeAction = ImeAction.Done),
        keyboardActions = KeyboardActions(onDone = { focus.clearFocus() }),
        modifier = Modifier.fillMaxWidth(), minLines = lines, maxLines = if (singleLine) 1 else 5, singleLine = singleLine)
}

internal val LocalAuthorProfilePresenter =
    staticCompositionLocalOf<(@Composable (onDismiss: () -> Unit, content: @Composable () -> Unit) -> Unit)?> { null }

@Composable
internal fun ProfileOverview(
    name: String,
    username: String?,
    profile: CommunityProfile?,
    own: Boolean = false,
    usernameAction: (@Composable () -> Unit)? = null,
) {
    val photo = profile?.photo
    val portrait = remember(photo) { photo?.takeIf { it.isNotEmpty() }?.let(::decodeProfilePhoto) }
    val intentions = remember(profile?.hereFor) { profile?.hereFor.orEmpty().distinct() }
    val colors = MaterialTheme.colorScheme
    val secondaryText = if (LocalFeedControl.current != null) colors.onSurface.copy(alpha = .75f)
        else colors.onSurfaceVariant
    val type = MaterialTheme.typography
    val nameplate: @Composable () -> Unit = {
        Column(verticalArrangement = Arrangement.spacedBy(4.dp)) {
            Text(name, Modifier.semantics { heading() }, style = type.headlineLarge, fontWeight = FontWeight.Bold)
            val pronouns = profile?.pronouns?.takeIf { it.isNotBlank() }
            if (pronouns != null || username != null) {
                FlowRow(horizontalArrangement = Arrangement.spacedBy(12.dp), verticalArrangement = Arrangement.spacedBy(2.dp)) {
                    pronouns?.let { Text(it, style = type.bodyMedium, color = secondaryText) }
                    username?.let { Text(it, style = type.bodyMedium, color = colors.primary) }
                }
            }
            usernameAction?.invoke()
        }
    }
    val description: @Composable (Modifier) -> Unit = { modifier ->
        Column(modifier, verticalArrangement = Arrangement.spacedBy(14.dp)) {
            if (portrait == null && own && profile != null) {
                if (LocalDensity.current.fontScale >= 1.6f) {
                    ProfilePortrait(name, profile)
                    nameplate()
                } else {
                    Row(verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(14.dp)) {
                        ProfilePortrait(name, profile)
                        Box(Modifier.weight(1f)) { nameplate() }
                    }
                }
            } else nameplate()
            if (intentions.isNotEmpty()) {
                Column(verticalArrangement = Arrangement.spacedBy(6.dp)) {
                    Text("Here for", Modifier.semantics { heading() }, style = type.labelLarge, color = secondaryText)
                    FlowRow(horizontalArrangement = Arrangement.spacedBy(6.dp), verticalArrangement = Arrangement.spacedBy(6.dp)) {
                        intentions.forEach { intention ->
                            Surface(color = colors.secondaryContainer, shape = CircleShape) {
                                Text(intention, Modifier.padding(horizontal = 12.dp, vertical = 6.dp),
                                    style = type.labelLarge, color = colors.onSecondaryContainer)
                            }
                        }
                    }
                }
            }
            profile?.bio?.takeIf { it.isNotBlank() }?.let { Text(it, style = postBodyStyle) }
            val identity = profile?.identity.orEmpty()
            val interests = profile?.interests.orEmpty()
            if (identity.isNotBlank() || interests.isNotBlank()) {
                Column(verticalArrangement = Arrangement.spacedBy(12.dp)) {
                    listOf("Identity" to identity, "Current joys" to interests).forEach { (label, value) ->
                        if (value.isNotBlank()) Column(verticalArrangement = Arrangement.spacedBy(2.dp)) {
                            Text(label, Modifier.semantics { heading() }, style = type.labelLarge, color = secondaryText)
                            Text(value, style = type.bodyLarge)
                        }
                    }
                }
            }
            if (!own) {
                val hasDetails = portrait != null || intentions.isNotEmpty() ||
                    listOf(profile?.pronouns, profile?.bio, identity, interests).any { !it.isNullOrBlank() }
                Text(if (hasDetails) "Current public profile" else "No further public profile details.",
                    style = type.bodySmall, color = secondaryText)
            }
        }
    }
    BoxWithConstraints(Modifier.fillMaxWidth()) {
        if (portrait == null) {
            description(Modifier.fillMaxWidth())
        } else {
            val photoContent: @Composable (Modifier) -> Unit = { modifier ->
                Image(portrait, "Profile photo of $name",
                    modifier.aspectRatio(1f).clip(MaterialTheme.shapes.large),
                    contentScale = ContentScale.Crop)
            }
            if (maxWidth >= 560.dp && LocalDensity.current.fontScale < 1.6f) {
                Row(horizontalArrangement = Arrangement.spacedBy(24.dp)) {
                    photoContent(Modifier.weight(5f))
                    description(Modifier.weight(6f))
                }
            } else {
                Column(verticalArrangement = Arrangement.spacedBy(14.dp)) {
                    photoContent(Modifier.widthIn(max = 384.dp).fillMaxWidth())
                    description(Modifier.fillMaxWidth())
                }
            }
        }
    }
}

@Composable
internal fun AuthorProfileSheet(name: String, username: String?, profile: CommunityProfile?, onDismiss: () -> Unit,
                                onMessage: (() -> Unit)? = null, onBlock: (() -> Unit)? = null) {
    val presenter = LocalAuthorProfilePresenter.current
    val content: @Composable () -> Unit = {
        val nativeControl = LocalFeedControl.current
        Column(Modifier.widthIn(max = 760.dp).fillMaxWidth()) {
            LazyColumn(Modifier.weight(1f), contentPadding = PaddingValues(start = 22.dp, end = 22.dp, top = 8.dp, bottom = 20.dp),
                verticalArrangement = Arrangement.spacedBy(12.dp)) {
                item { ProfileOverview(name, username, profile) }
                if (onBlock != null) item {
                    if (nativeControl != null) {
                        nativeControl(FormRow("block", "Block account", destructive = true, onTap = onBlock), Modifier)
                    } else {
                        TextButton(onClick = onBlock, colors = ButtonDefaults.textButtonColors(contentColor = MaterialTheme.colorScheme.error)) {
                            Text("Block account")
                        }
                    }
                }
            }
            if (onMessage != null) {
                HorizontalDivider()
                Box(Modifier.fillMaxWidth().padding(horizontal = 22.dp, vertical = 10.dp)) {
                    val button = Modifier.fillMaxWidth()
                    if (nativeControl != null) {
                        nativeControl(FormRow("message", "Message", "primary", symbol = "envelope", onTap = onMessage), button)
                    } else {
                        YapButton(onClick = onMessage, modifier = button) { Text("Message") }
                    }
                }
            }
        }
    }
    if (presenter != null) {
        presenter(onDismiss, content)
    } else {
        ModalBottomSheet(onDismissRequest = onDismiss,
            modifier = Modifier.fillMaxHeight().statusBarsPadding(),
            sheetState = rememberModalBottomSheetState(skipPartiallyExpanded = true)) {
            Row(Modifier.fillMaxWidth().padding(horizontal = 12.dp), horizontalArrangement = Arrangement.End) {
                YapCancelButton(onClick = onDismiss, label = "Done")
            }
            content()
        }
    }
}

@Composable
internal fun ProfileSection(title: String, rows: @Composable ColumnScope.() -> Unit) {
    Column(verticalArrangement = Arrangement.spacedBy(8.dp)) {
        Text(title, style = MaterialTheme.typography.titleLarge)
        Surface(shape = MaterialTheme.shapes.large, color = MaterialTheme.colorScheme.surface) {
            Column(verticalArrangement = Arrangement.spacedBy(2.dp), content = rows)
        }
    }
}

@Composable
internal fun ProfileOption(label: String, icon: String, expanded: Boolean? = null, external: Boolean = false, onClick: () -> Unit) {
    val colors = MaterialTheme.colorScheme
    ListItem(
        modifier = Modifier.fillMaxWidth().clickable(role = androidx.compose.ui.semantics.Role.Button, onClick = onClick)
            .semantics {
                if (expanded != null) stateDescription = if (expanded) "Expanded" else "Collapsed"
            },
        headlineContent = { Text(label) },
        leadingContent = { LineIcon(icon, null, tint = colors.primary) },
        trailingContent = {
            val indicator = when {
                external -> "external"
                expanded == null -> "forward"
                expanded -> "up"
                else -> "down"
            }
            LineIcon(indicator, null, Modifier.size(20.dp), tint = colors.onSurfaceVariant)
        },
        colors = ListItemDefaults.colors(containerColor = colors.surfaceContainer)
    )
}
