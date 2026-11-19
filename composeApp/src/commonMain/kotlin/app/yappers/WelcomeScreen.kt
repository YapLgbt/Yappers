package app.yappers

import androidx.compose.foundation.layout.*
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.foundation.text.selection.SelectionContainer
import androidx.compose.ui.platform.LocalUriHandler
import androidx.compose.ui.platform.LocalClipboardManager
import androidx.compose.ui.text.LinkAnnotation
import androidx.compose.ui.text.buildAnnotatedString
import androidx.compose.ui.text.withLink
import androidx.compose.ui.text.AnnotatedString
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.input.KeyboardType
import androidx.compose.ui.text.input.PasswordVisualTransformation
import androidx.compose.ui.unit.dp
import kotlinx.coroutines.CancellationException
import kotlinx.coroutines.launch

private const val TERMS_URL = "https://yap.lgbt/terms/"
private const val PRIVACY_URL = "https://yap.lgbt/privacy-policy/"

@Composable
private fun WelcomePolicyFooter() {
    val uriHandler = LocalUriHandler.current
    val control = LocalFeedControl.current
    Row(Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.Center, verticalAlignment = Alignment.CenterVertically) {
        for ((title, url) in listOf("Terms of Service" to TERMS_URL, "Privacy Policy" to PRIVACY_URL)) {
            Box(Modifier.weight(1f), contentAlignment = Alignment.Center) {
                if (control != null) control(FormRow(url, title, "link", identifier = title) { uriHandler.openUri(url) }, Modifier)
                else Text(
                    buildAnnotatedString {
                        withLink(LinkAnnotation.Url(url)) { append(title) }
                    },
                    modifier = Modifier.padding(vertical = 12.dp), color = MaterialTheme.colorScheme.primary,
                    style = MaterialTheme.typography.labelLarge
                )
            }
        }
    }
}

@Composable
internal fun PolicyLinks() {
    val uriHandler = LocalUriHandler.current
    val native = LocalNativeForm.current
    if (native != null) {
        native(NativeFormModel(sections = listOf(FormSection(rows = listOf(
            FormRow("terms", "Terms of Service") { uriHandler.openUri(TERMS_URL) },
            FormRow("privacy", "Privacy Policy") { uriHandler.openUri(PRIVACY_URL) },
        )))), "inline", Modifier.fillMaxWidth())
        return
    }
    FlowRow {
        TextButton(onClick = { uriHandler.openUri(TERMS_URL) }) { Text("Terms of Service") }
        TextButton(onClick = { uriHandler.openUri(PRIVACY_URL) }) { Text("Privacy Policy") }
    }
}

@Composable
internal fun WelcomeScreen(controller: NetkitController, files: RecoveryFiles?, photoLibrary: ProfilePhotoLibrary?) {
    var step by remember { mutableStateOf("welcome") }
    var name by remember { mutableStateOf(suggestName()) }
    var key by remember { mutableStateOf("") }
    var file by remember { mutableStateOf<String?>(null) }
    var passphrase by remember { mutableStateOf("") }
    var busy by remember { mutableStateOf(false) }
    var error by remember { mutableStateOf<String?>(null) }
    val scope = rememberCoroutineScope()
    fun go(to: String) {
        step = to
        error = null
        key = ""
        passphrase = ""
        file = null
    }
    AppBackHandler(step != "welcome") { if (!busy) go(if (step == "file" || step == "key") "restore" else "welcome") }
    fun finish() {
        if (busy) return
        busy = true
        error = null
        scope.launch {
            try {
                val recovery = when (step) {
                    "restore", "key" -> key.trim()
                    "file" -> controller.keyFromBackup(checkNotNull(file), passphrase)
                    else -> null
                }
                controller.finishWelcome(name.trim().ifEmpty { suggestName() }, recovery)
            } catch (cancelled: CancellationException) { throw cancelled }
            catch (failure: Exception) { error = failure.message ?: "Couldn’t connect to your home. Try again." }
            finally { busy = false }
        }
    }
    val native = LocalNativeForm.current
    val vault = files as? RecoveryKeyVault
    val control = LocalFeedControl.current
    val nativeRestore = native != null && control != null && step in listOf("restore", "key", "file")
    fun chooseKey() {
        vault?.chooseKey { saved, problem ->
            if (saved != null) {
                if (nativeRestore) go("key")
                key = saved
            }
            error = problem
        }
    }
    fun chooseBackup() {
        files?.loadRecovery { value, problem ->
            if (value != null) file = value
            error = problem
        }
    }
    if (step == "name" && native != null && control != null) {
        ProfileEditor(name, CommunityProfile(), photoLibrary, creating = true, onDismiss = { go("welcome") }) { alias, profile ->
            controller.finishWelcome(alias, profile = profile)
        }
        return
    }
    EstuaryOnboardingSurface(
        header = if (nativeRestore) ({
            control(FormRow("back", "Back", "back", symbol = "chevron.backward", enabled = !busy) { go("welcome") }, Modifier)
        }) else null,
        footer = if (step == "welcome") ({ WelcomePolicyFooter() }) else null
    ) {
        if (step == "welcome") {
            YapWordmark(Modifier.align(Alignment.CenterHorizontally), prominent = true)
            Text("Welcome to Yap", style = MaterialTheme.typography.headlineLarge, fontWeight = FontWeight.Bold)
            Text("Yap is a neighborhood for queer conversation. Join yap.lgbt to share a plan, ask a question or find your people.")
            Text("For ages 18 and older.", style = MaterialTheme.typography.bodyMedium, color = MaterialTheme.colorScheme.onSurfaceVariant)
            if (control != null) {
                control(FormRow("create", "Create an account", "primary", identifier = "welcome-create") { go("name") }, Modifier.fillMaxWidth())
                control(FormRow("restore", "Restore an existing account", "secondary", identifier = "welcome-restore") { go("restore") }, Modifier.align(Alignment.CenterHorizontally))
            } else {
                Button(onClick = { go("name") }, modifier = Modifier.fillMaxWidth()) { Text("Create an account") }
                OutlinedButton(onClick = { go("restore") }, modifier = Modifier.align(Alignment.CenterHorizontally)) { Text("Restore an existing account") }
            }
            return@EstuaryOnboardingSurface
        }
        if (nativeRestore) {
            YapWordmark(Modifier.align(Alignment.CenterHorizontally), prominent = true)
            Text("Welcome back", style = MaterialTheme.typography.headlineLarge, fontWeight = FontWeight.Bold)
            Text("Choose how to restore your Yap account.")
            Column(Modifier.fillMaxWidth()) {
                if (vault != null) {
                    control(FormRow("keychain", "Use a key from Keychain", "method",
                        detail = "Find a key you saved with Apple.", symbol = "key.icloud", onTap = ::chooseKey), Modifier.fillMaxWidth())
                    HorizontalDivider(color = MaterialTheme.colorScheme.outlineVariant)
                }
                control(FormRow("manual", "Enter a recovery key", "method",
                    detail = "Paste or type your saved key.", symbol = "key") { go("key") }, Modifier.fillMaxWidth())
                if (files != null) {
                    HorizontalDivider(color = MaterialTheme.colorScheme.outlineVariant)
                    control(FormRow("backup", "Use an encrypted app backup", "method",
                        detail = "Open a backup file with its passphrase.", symbol = "doc.badge.clock") { go("file") }, Modifier.fillMaxWidth())
                }
            }
            if (step == "restore") {
                error?.let { Text(it, color = MaterialTheme.colorScheme.error) }
            } else {
                val fromFile = step == "file"
                val rows = buildList {
                    if (fromFile) {
                        add(FormRow("choose-file", if (file == null) "Choose backup file" else "Change backup file",
                            symbol = "folder", enabled = !busy, onTap = ::chooseBackup))
                        add(FormRow("passphrase", "Backup passphrase", "secure", value = passphrase,
                            placeholder = "Enter passphrase", enabled = !busy, onChange = { passphrase = it }))
                    } else {
                        add(FormRow("key", "Recovery key", "secure", value = key, limit = 128,
                            placeholder = "Enter recovery key", enabled = !busy, onChange = { key = it }))
                    }
                    error?.let { add(FormRow("error", it, "text")) }
                }
                val cancel = { if (!busy) go("restore") }
                native(NativeFormModel(title = if (fromFile) "Encrypted backup" else "Recovery key",
                    sections = listOf(FormSection(rows = rows)),
                    leading = FormChoice("Cancel", action = cancel), leadingEnabled = !busy,
                    trailing = FormChoice(if (busy) "Restoring…" else "Restore", action = ::finish),
                    trailingEnabled = !busy && (if (fromFile) file != null && passphrase.isNotEmpty() else key.isNotBlank()),
                    canDismiss = !busy, onDismiss = cancel), "compactSheet", Modifier)
            }
            return@EstuaryOnboardingSurface
        }
        YapWordmark()
        Text(when (step) {
            "name" -> "What should neighbors call you?"
            else -> "Welcome back"
        }, style = MaterialTheme.typography.headlineLarge, fontWeight = FontWeight.Bold)
        when (step) {
            "name" -> {
                Text("Use a name you’re comfortable posting with. You can also post anonymously.")
                OutlinedTextField(name, { if (it.length <= 30) name = it }, label = { Text("Display name (optional)") },
                    singleLine = true, enabled = !busy, modifier = Modifier.fillMaxWidth())
                TextButton(enabled = !busy, onClick = { name = suggestName(name) }) { Text("Suggest another name") }
                Button(enabled = !busy && name.trim().length != 1, onClick = ::finish) { Text(if (busy) "Joining…" else "Join yap.lgbt") }
            }
            "restore" -> {
                Text("Use your saved recovery key to add this device to your account.")
                if (vault != null) OutlinedButton(enabled = !busy, onClick = ::chooseKey) { Text(if (vault is PasswordManagerVault) "Use a saved recovery key" else "Use a key from Keychain") }
                OutlinedTextField(key, { if (it.length <= 128) key = it }, label = { Text("Recovery key") },
                    singleLine = true, enabled = !busy, visualTransformation = PasswordVisualTransformation(),
                    keyboardOptions = KeyboardOptions(keyboardType = KeyboardType.Password), modifier = Modifier.fillMaxWidth())
                Button(enabled = !busy && key.isNotBlank(), onClick = ::finish) { Text(if (busy) "Restoring…" else "Restore account") }
                if (files != null) TextButton(enabled = !busy, onClick = { go("file") }) { Text("Use an encrypted app backup") }
            }
            "file" -> {
                Text("Choose a backup exported from the Yap app to restore account access. Keep the file for your saved history.")
                OutlinedButton(enabled = !busy, onClick = ::chooseBackup) { Text(if (file == null) "Choose backup file" else "Change backup file") }
                OutlinedTextField(passphrase, { passphrase = it }, label = { Text("Backup passphrase") },
                    singleLine = true, enabled = !busy, visualTransformation = PasswordVisualTransformation(),
                    keyboardOptions = KeyboardOptions(keyboardType = KeyboardType.Password), modifier = Modifier.fillMaxWidth())
                Button(enabled = !busy && file != null && passphrase.isNotEmpty(), onClick = ::finish) { Text(if (busy) "Restoring…" else "Restore account") }
            }
        }
        error?.let { Text(it, color = MaterialTheme.colorScheme.error) }
        if (step != "welcome") TextButton(enabled = !busy, onClick = { go(if (step == "file" || step == "key") "restore" else "welcome") }) { Text("Back") }
    }
}

@Composable
internal fun WelcomeBackupScreen(controller: NetkitController, accountReady: Boolean, replay: Boolean, returningName: String? = null,
    vault: RecoveryKeyVault? = null, onSaveName: suspend (String) -> Unit = {}, onDone: () -> Unit) {
    var step by remember { mutableStateOf(if (replay || returningName != null) "intro" else "backup") }
    var name by remember { mutableStateOf(returningName.orEmpty()) }
    val scope = rememberCoroutineScope()
    var key by remember { mutableStateOf<String?>(null) }
    var error by remember { mutableStateOf<String?>(null) }
    var busy by remember { mutableStateOf(false) }
    fun act(block: suspend () -> Unit) {
        if (busy) return
        busy = true
        error = null
        scope.launch {
            try { block() }
            catch (cancelled: CancellationException) { throw cancelled }
            catch (problem: Exception) { error = problem.message ?: "Check your connection and try again." }
            finally { busy = false }
        }
    }
    fun saveName() = act {
        if (name.trim() != returningName) onSaveName(name.trim())
        step = "backup"
    }
    fun getKey() = act { key = controller.recoveryKey() }
    fun enter() = act { if (!replay) controller.enterNeighborhood(); onDone() }
    val native = LocalNativeForm.current
    EstuaryOnboardingSurface {
        val control = LocalFeedControl.current
        if (step == "backup" && control != null) {
            YapWordmark(Modifier.align(Alignment.CenterHorizontally), prominent = true)
            Text("Keep your account", style = MaterialTheme.typography.headlineLarge, fontWeight = FontWeight.Bold)
            Text("Save your recovery key to use this account on another device.")
            control(FormRow("key", "Get recovery key", "primary", enabled = accountReady && !busy, onTap = ::getKey), Modifier.fillMaxWidth())
            control(FormRow("enter", "Enter neighborhood", "secondary", enabled = !busy, onTap = ::enter), Modifier.align(Alignment.CenterHorizontally))
            Text("You can back up later in You → Account recovery.", style = MaterialTheme.typography.bodyMedium)
            error?.let { Text(it, color = MaterialTheme.colorScheme.error) }
            return@EstuaryOnboardingSurface
        }
        if (control != null) YapWordmark(Modifier.align(Alignment.CenterHorizontally), prominent = true) else YapWordmark()
        if (step == "intro") {
            Text("Welcome to Yap", style = MaterialTheme.typography.headlineLarge, fontWeight = FontWeight.Bold)
            Text("Your account is already on this device. You can post by name or anonymously in your neighborhood.")
            val proceed = { step = if (returningName == null) "backup" else "name" }
            if (control != null) {
                control(FormRow("continue", "Continue with my account", "primary", onTap = proceed), Modifier.fillMaxWidth())
                control(FormRow("return", "Return to neighborhood", "secondary", onTap = onDone), Modifier.align(Alignment.CenterHorizontally))
            } else {
                Button(onClick = proceed) { Text("Continue with my account") }
                TextButton(onClick = onDone) { Text("Return to neighborhood") }
            }
        } else if (step == "name") {
            Text("What should neighbors call you?", style = MaterialTheme.typography.headlineLarge, fontWeight = FontWeight.Bold)
            Text("Keep your display name, pick a suggestion, or write your own.")
            val valid = !busy && name.trim().length in 2..30
            if (native != null && control != null) {
                val field = FormRow("name", "Display name", "field", value = name, enabled = !busy, limit = 30,
                    onChange = { if (it.length <= 30) name = it })
                native(NativeFormModel(sections = listOf(FormSection(rows = listOf(field)))), "input", Modifier.fillMaxWidth())
                control(FormRow("suggest", "Suggest another name", "secondary", enabled = !busy) { name = suggestName(name) }, Modifier)
                control(FormRow("continue", "Continue", "primary", enabled = valid, onTap = ::saveName), Modifier.fillMaxWidth())
            } else {
                OutlinedTextField(name, { if (it.length <= 30) name = it }, label = { Text("Display name") },
                    enabled = !busy, singleLine = true, modifier = Modifier.fillMaxWidth())
                TextButton(enabled = !busy, onClick = { name = suggestName(name) }) { Text("Suggest another name") }
                Button(enabled = valid, onClick = ::saveName) { Text("Continue") }
            }
            error?.let { Text(it, color = MaterialTheme.colorScheme.error) }
        } else {
            Text("Keep your account", style = MaterialTheme.typography.headlineLarge, fontWeight = FontWeight.Bold)
            Text("Your account is ready. Save your recovery key to use this account on another device.")
            OutlinedButton(enabled = accountReady && !busy, onClick = ::getKey) { Text("Get recovery key") }
            Button(enabled = !busy, onClick = ::enter) { Text("Enter neighborhood") }
            Text("You can back up later in You → Account recovery.", style = MaterialTheme.typography.bodyMedium)
            error?.let { Text(it, color = MaterialTheme.colorScheme.error) }
        }
    }
    key?.let { RecoveryKeyDialog(it, controller.state.value.snapshot.alias, vault) { key = null } }
}

@Composable
internal fun RecoveryKeyDialog(key: String, alias: String, vault: RecoveryKeyVault?, onClose: () -> Unit) {
    @Suppress("DEPRECATION") val clipboard = LocalClipboardManager.current
    var revealed by remember(key) { mutableStateOf(false) }
    var saved by remember(key, vault) { mutableStateOf(false) }
    LaunchedEffect(key, vault) {
        saved = kotlinx.coroutines.withContext(kotlinx.coroutines.Dispatchers.Default) { vault?.hasKey(key) == true }
    }
    var removing by remember { mutableStateOf(false) }
    var status by remember { mutableStateOf<String?>(null) }
    fun copyKey() {
        clipboard.setText(AnnotatedString(key))
        status = "Copied. Paste it into your password manager."
    }
    fun saveKey() {
        vault?.saveKey(key, alias) { problem ->
            saved = problem == null
            status = problem
        }
    }
    fun removeKey() {
        vault?.removeKey(key) { problem ->
            if (problem == null) saved = false
            status = problem
        }
        removing = false
    }
    val native = LocalNativeForm.current
    if (native != null) {
        val rows = buildList {
            if (vault != null) {
                if (saved) {
                    add(FormRow("saved", "Saved to Keychain", "text"))
                    add(FormRow("remove", "Remove saved key", destructive = true) { removing = true })
                } else add(FormRow("save", "Save to Keychain", onTap = ::saveKey))
            }
            if (revealed) add(FormRow("revealed-key", "Recovery key", "selectable", value = key))
            add(FormRow("show", if (revealed) "Hide key" else "Show key") { revealed = !revealed })
            add(FormRow("copy", "Copy key", onTap = ::copyKey))
            status?.let { add(FormRow("status", it, "text")) }
        }
        native(NativeFormModel(title = "Your recovery key", sections = listOf(
            FormSection(rows = listOf(FormRow("key-guidance", "Anyone with this key can use your account. Keep it somewhere private.", "text"))),
            FormSection(rows = rows, footer = if (vault == null) "" else "With iCloud Keychain on, you can restore from Yap on another iPhone."),
        ), trailing = FormChoice("Done", action = onClose), onDismiss = onClose,
            confirmation = if (removing) FormConfirmation("Remove this recovery key from Keychain?",
                "If you use iCloud Keychain, this removes the saved key from your other devices too.",
                FormChoice("Remove key", destructive = true, action = ::removeKey), FormChoice("Cancel") { removing = false }) else null),
            "sheet", Modifier)
        return
    }
    AlertDialog(onDismissRequest = onClose, title = { Text("Your recovery key") },
        text = { Column(verticalArrangement = Arrangement.spacedBy(8.dp)) {
            Text("Anyone with this key can use your account. Keep it somewhere private.")
            if (vault != null) {
                if (saved) {
                    Text(if (vault is PasswordManagerVault) "Saved to password manager" else "Saved to Keychain")
                    if (vault !is PasswordManagerVault) TextButton(onClick = { removing = true }) { Text("Remove saved key") }
                } else {
                    Button(onClick = ::saveKey) { Text(if (vault is PasswordManagerVault) "Save to password manager" else "Save to Keychain") }
                }
                if (vault !is PasswordManagerVault) Text("With iCloud Keychain on, you can restore from Yap on another iPhone.", style = MaterialTheme.typography.bodySmall)
            }
            if (revealed) SelectionContainer { Text(key) }
            TextButton(onClick = { revealed = !revealed }) { Text(if (revealed) "Hide key" else "Show key") }
            status?.let { Text(it) }
        } }, dismissButton = { TextButton(onClick = ::copyKey) { Text("Copy key") } }, confirmButton = { TextButton(onClick = onClose) { Text("Done") } })
    if (removing && vault != null) AlertDialog(onDismissRequest = { removing = false },
        title = { Text("Remove this recovery key from Keychain?") },
        text = { Text("If you use iCloud Keychain, this removes the saved key from your other devices too.") },
        confirmButton = { TextButton(onClick = ::removeKey) { Text("Remove key") } },
        dismissButton = { TextButton(onClick = { removing = false }) { Text("Cancel") } })
}

@Composable
internal fun WelcomeUsernameScreen(controller: NetkitController, connection: NetkitState, status: WelcomeUsername,
                                   saveProfile: (String, CommunityProfile) -> Unit) {
    var name by remember(connection.messagingScope) { mutableStateOf("") }
    var error by remember(connection.messagingScope) { mutableStateOf<String?>(null) }
    var working by remember { mutableStateOf(false) }
    val scope = rememberCoroutineScope()
    val claim = rememberEditorUsernameClaim(connection, controller)
    suspend fun verify(retry: Boolean) {
        working = true
        error = null
        try {
            if (retry && !controller.state.value.enabled) controller.restore()
            if (controller.state.value.enabled) {
                controller.persistWelcomeProfile(saveProfile)
                controller.checkWelcomeUsername(retry)
            }
        } catch (cancelled: CancellationException) { throw cancelled }
        catch (failure: Exception) { error = failure.message ?: "Couldn’t verify your username. Try again." }
        finally { working = false }
    }
    LaunchedEffect(controller, connection.messagingScope) {
        if (connection.enabled && status == WelcomeUsername.Unchecked) verify(false)
    }
    fun submit() {
        if (working || claim == null || !claim.isValid(name)) return
        working = true
        error = null
        claim.submit(name) { problem -> working = false; error = problem }
    }
    val registration = claim?.takeIf { status == WelcomeUsername.Required }
    val help = when {
        registration != null -> "Your permanent address ends in @${registration.domain}. Use 2–24 letters, numbers, or underscores. Start with a letter."
        status == WelcomeUsername.Unsupported -> "This community does not offer usernames. You need a username to finish setup. Check again after your community enables them."
        else -> "Connecting to your community to verify your account’s username."
    }
    val retry = { if (!working) scope.launch { verify(true) }; Unit }
    EstuaryOnboardingSurface {
        YapWordmark(Modifier.align(Alignment.CenterHorizontally), prominent = true)
        Text(if (registration != null) "Choose your username" else "Verify your username", style = MaterialTheme.typography.headlineLarge, fontWeight = FontWeight.Bold)
        val native = LocalNativeForm.current
        val control = LocalFeedControl.current
        if (native != null && control != null) {
            Text(help)
            if (registration != null) {
                val field = FormRow("name", "Username", "field", value = name, placeholder = "Enter a username", limit = 24,
                    keyboard = "ascii", enabled = !working, identifier = "onboarding-username", onChange = { name = registration.normalize(it); error = null })
                native(NativeFormModel(sections = listOf(FormSection(rows = listOf(field)))), "input", Modifier.fillMaxWidth())
                control(FormRow("claim", if (working) "Claiming…" else "Claim username", "primary",
                    enabled = !working && registration.isValid(name), onTap = ::submit), Modifier.fillMaxWidth())
            } else if (!working) control(FormRow("retry", "Check again", "secondary", onTap = retry), Modifier)
            error?.let { Text(it, color = MaterialTheme.colorScheme.error) }
        } else {
            Text(help)
            if (registration != null) {
                OutlinedTextField(name, { name = registration.normalize(it); error = null }, label = { Text("Username") },
                    singleLine = true, enabled = !working, keyboardOptions = KeyboardOptions(autoCorrectEnabled = false, keyboardType = KeyboardType.Ascii))
                Button(enabled = !working && registration.isValid(name), onClick = ::submit) { Text(if (working) "Claiming…" else "Claim username") }
            } else if (!working) TextButton(onClick = retry) { Text("Check again") }
            error?.let { Text(it, color = MaterialTheme.colorScheme.error) }
        }
    }
}

private val usernamePattern = Regex("[a-z][a-z0-9_]{1,23}")
internal fun validUsername(name: String) = usernamePattern.matches(name)

/** The same name rules and registration operation back the profile and native writing forms. */
class EditorUsernameClaim internal constructor(
    val domain: String,
    private val register: (String, (String?) -> Unit) -> Unit
) {
    fun normalize(name: String) = name.lowercase().take(24)
    fun isValid(name: String) = validUsername(name)
    fun submit(name: String, completed: (String?) -> Unit) {
        if (isValid(name)) register(name, completed)
        else completed("Use 2–24 letters, numbers, or underscores. Start with a letter.")
    }
}

@Composable
internal fun rememberEditorUsernameClaim(connection: NetkitState, controller: NetkitController?): EditorUsernameClaim? {
    val scope = rememberCoroutineScope()
    val domain = connection.usernameDomain ?: return null
    if (!connection.needsUsername || controller == null) return null
    val home = connection.messagingScope
    return remember(domain, home, controller, scope) {
        EditorUsernameClaim(domain) { name, completed ->
            scope.launch {
                val error = try {
                    val current = controller.state.value
                    check(current.needsUsername && current.usernameDomain == domain && current.messagingScope == home) {
                        "Your community changed. Reopen the username form."
                    }
                    controller.claimUsername(name)
                    null
                } catch (cancelled: CancellationException) {
                    throw cancelled
                } catch (problem: Exception) {
                    problem.message ?: "Couldn’t claim that username. Try again."
                }
                completed(error)
            }
        }
    }
}

@Composable
internal fun UsernameControl(connection: NetkitState, controller: NetkitController?) {
    val domain = connection.usernameDomain ?: return
    if (!connection.enabled || controller == null) return
    connection.privacy.username?.let { Text(it, color = MaterialTheme.colorScheme.onSurfaceVariant); return }
    var open by remember { mutableStateOf(false) }
    var name by remember { mutableStateOf("") }
    var error by remember { mutableStateOf<String?>(null) }
    var busy by remember { mutableStateOf(false) }
    val claim = rememberEditorUsernameClaim(connection, controller) ?: return
    fun submit() {
        if (busy || !claim.isValid(name)) return
        busy = true
        error = null
        claim.submit(name) { problem ->
            busy = false
            error = problem
            if (problem == null) open = false
        }
    }
    val native = LocalNativeForm.current
    if (native != null) {
        native(NativeFormModel(sections = listOf(FormSection(rows = listOf(
            FormRow("choose-username", "Choose a username") { name = ""; error = null; open = true }
        )))), "inline", Modifier.fillMaxWidth())
        if (open) native(NativeFormModel(title = "Choose a username", sections = listOf(FormSection(rows = buildList {
            add(FormRow("username", "Username", "field", value = name, limit = 24, keyboard = "ascii", enabled = !busy,
                identifier = "editor-claim-username", onChange = { name = claim.normalize(it); error = null }))
            error?.let { add(FormRow("error", it, "text")) }
        }, footer = "Your permanent address ends in @$domain and appears on named posts and replies. Use 2–24 letters, numbers, or underscores. Start with a letter.")),
            leading = FormChoice("Cancel") { if (!busy) open = false }, leadingEnabled = !busy, trailing = FormChoice("Claim username", action = ::submit),
            trailingEnabled = !busy && claim.isValid(name), canDismiss = !busy, onDismiss = { if (!busy) open = false }),
            "sheet", Modifier)
        return
    }
    TextButton(onClick = { error = null; open = true }) { Text("Choose a username") }
    if (open) AlertDialog(onDismissRequest = { if (!busy) open = false }, title = { Text("Choose a username") },
        text = { Column(verticalArrangement = Arrangement.spacedBy(10.dp)) {
            Text("This permanent address appears under your display name on named posts and replies.")
            OutlinedTextField(name, { name = claim.normalize(it); error = null }, label = { Text("Username") },
                suffix = { Text("@$domain") }, singleLine = true, enabled = !busy,
                keyboardOptions = KeyboardOptions(autoCorrectEnabled = false, keyboardType = KeyboardType.Ascii))
            Text("Use 2–24 letters, numbers, or underscores. Start with a letter.", style = MaterialTheme.typography.bodySmall)
            error?.let { Text(it, color = MaterialTheme.colorScheme.error) }
        } }, dismissButton = { TextButton(enabled = !busy, onClick = { open = false }) { Text("Cancel") } },
        confirmButton = { Button(enabled = !busy && claim.isValid(name), onClick = ::submit) { Text(if (busy) "Claiming…" else "Claim username") } })
}
