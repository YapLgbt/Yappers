package app.yappers

import androidx.compose.ui.test.*
import androidx.compose.ui.test.junit4.v2.createComposeRule
import androidx.compose.ui.test.v2.runSkikoComposeUiTest
import androidx.compose.foundation.layout.*
import androidx.compose.runtime.mutableFloatStateOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.ui.Modifier
import androidx.compose.ui.geometry.Size
import androidx.compose.ui.MotionDurationScale
import androidx.compose.ui.unit.dp
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.semantics.SemanticsActions
import androidx.compose.ui.semantics.SemanticsProperties
import androidx.compose.ui.graphics.asSkiaBitmap
import androidx.compose.ui.text.TextLayoutResult
import kotlinx.coroutines.runBlocking
import kotlinx.serialization.json.*
import org.junit.Rule
import org.junit.Test
import java.nio.file.Files
import java.nio.file.Path
import kotlin.test.*
import kotlin.time.Duration.Companion.seconds

/** Real desktop UI and C-ABI coverage, alongside the shared controller/domain tests. */
class DesktopTest {
    // Keep the continuously animated flag still for deterministic screenshots and UI actions.
    private val motionScale = mutableFloatStateOf(0f)
    @get:Rule val compose = createComposeRule(object : MotionDurationScale {
        override val scaleFactor get() = motionScale.floatValue
    })

    @Test fun votingKeepsTheScoreCenteredAcrossDigitBoundaries() {
        val neutral = mutableStateOf(9)
        val vote = mutableStateOf(0)
        compose.setContent {
            YapTheme {
                VoteControl(neutral.value + vote.value, vote.value) { direction ->
                    vote.value = if (vote.value == direction) 0 else direction
                }
            }
        }
        fun center(score: Int): Float {
            val number = compose.onNodeWithText(score.toString()).fetchSemanticsNode().boundsInRoot.center.x
            val up = compose.onNodeWithContentDescription("Upvote").fetchSemanticsNode().boundsInRoot.center.x
            val down = compose.onNodeWithContentDescription("Downvote").fetchSemanticsNode().boundsInRoot.center.x
            assertEquals((up + down) / 2f, number, 1f)
            return number
        }
        for (base in listOf(9, 99, 9999, -10, -1000)) {
            compose.runOnIdle { neutral.value = base }
            val start = center(base)
            compose.onNodeWithContentDescription("Upvote").performClick()
            assertEquals(start, center(base + 1), 1f)
            compose.onNodeWithContentDescription("Downvote").performClick()
            assertEquals(start, center(base - 1), 1f)
            compose.onNodeWithContentDescription("Downvote").performClick()
            assertEquals(start, center(base), 1f)
        }
    }

    @Test fun onboardingFlagFollowsSystemMotionPreference() {
        compose.mainClock.autoAdvance = false
        var systemReducesMotion = false
        compose.setContent {
            systemReducesMotion = platformReducesMotion()
            Box(Modifier.size(180.dp).testTag("flag")) { EstuaryOnboardingSurface {} }
        }
        fun frame(): ByteArray = org.jetbrains.skia.Image.makeFromBitmap(
            compose.onNodeWithTag("flag").captureToImage().asSkiaBitmap()).use { image -> image.encodeToData()!!.use { it.bytes } }
        compose.mainClock.advanceTimeByFrame()
        compose.onNodeWithText("Play motion").assertDoesNotExist()
        compose.onNodeWithText("Pause motion").assertDoesNotExist()
        val still = frame()
        compose.mainClock.advanceTimeBy(800)
        assertContentEquals(still, frame())
        compose.runOnIdle { motionScale.floatValue = 1f }
        compose.mainClock.advanceTimeBy(1600)
        if (!systemReducesMotion) assertFalse(still.contentEquals(frame()), "The fabric moves when system motion is enabled")
        compose.runOnIdle { motionScale.floatValue = 0f }
        compose.mainClock.advanceTimeByFrame()
        val reduced = frame()
        compose.mainClock.advanceTimeBy(800)
        assertContentEquals(reduced, frame())
    }

    @Test fun onboardingRequiresAUsernameCapableHome() {
        val fixture = System.getenv("YAPPERS_DESKTOP_FIXTURE") ?: return
        val home = Files.createDirectories(Path.of(fixture).resolve("legacy-welcome"))
        val manifest = home.resolve("enroll.json")
        val binary = Path.of(fixture).parent.resolve("rust/debug/yappers-server")
        val service = ProcessBuilder(binary.toString(), "public-home", "--directory", home.toString(), "--manifest", manifest.toString())
            .redirectErrorStream(true).redirectOutput(home.resolve("service.log").toFile()).start()
        try {
            compose.waitUntil(30_000) { Files.exists(manifest) }
            DesktopStorage(home.resolve("device")).use { storage ->
                val controller = NetkitController(storage, DesktopNetkitEngine()) { Files.readString(manifest) }
                controller.prepareWelcome(false)
                runBlocking { controller.finishWelcome("Reader") }
                compose.setContent { YappersApp(storage.local, controller, desktop = true, syncInView = false) }
                compose.waitUntil(30_000) { controller.welcomeUsername.value == WelcomeUsername.Unsupported }
                compose.onNodeWithText("This community does not offer usernames.", substring = true).assertIsDisplayed()
                compose.onNodeWithText("Check again").performClick()
                compose.waitUntil(30_000) { compose.onAllNodesWithText("Check again").fetchSemanticsNodes().isNotEmpty() }
                compose.onNodeWithText("Enter neighborhood").assertDoesNotExist()
                runBlocking {
                    assertFailsWith<IllegalStateException> { controller.enterNeighborhood() }
                    controller.shutdown()
                }
            }
        } finally { service.destroy(); service.waitFor() }
    }

    @Test fun welcomeCreatesAndRestoresIndependentDevices() {
        val fixture = System.getenv("YAPPERS_DESKTOP_FIXTURE") ?: return
        val home = Path.of(fixture).resolve("welcome")
        val publicDocument = home.resolve("enroll.json")
        Files.createDirectories(home)
        val binary = Path.of(fixture).parent.resolve("rust/debug/yappers-server")
        fun service(manifest: Path) = ProcessBuilder(binary.toString(), "public-home", "--directory", home.toString(), "--manifest", manifest.toString())
            .redirectErrorStream(true).redirectOutput(home.resolve("service.log").toFile())
        val seed = home.resolve("seed.json")
        val setup = service(seed).start()
        val id = try {
            compose.waitUntil(30_000) { Files.exists(seed) }
            Json.parseToJsonElement(Files.readString(seed)).jsonObject.getValue("join").jsonObject
                .getValue("serverAddr").jsonObject.getValue("id").jsonPrimitive.content
        } finally { setup.destroy(); setup.waitFor() }
        val service = service(publicDocument).apply { environment()["YAP_USERNAME_DOMAINS"] = "$id=neighborhood.test" }.start()
        try {
            compose.waitUntil(30_000) { Files.exists(publicDocument) }
            DesktopStorage(home.resolve("first")).use { first ->
                val engine = DesktopNetkitEngine()
                val calls = java.util.concurrent.ConcurrentLinkedQueue<String>()
                val observed = object : NetkitEngine by engine {
                    override suspend fun request(handle: Long, command: JsonObject): JsonObject {
                        calls.add(command.getValue("type").jsonPrimitive.content)
                        return engine.request(handle, command)
                    }
                }
                val controller = NetkitController(first, observed) { Files.readString(publicDocument) }
                controller.prepareWelcome(false)
                compose.setContent { YappersApp(first.local, controller, desktop = true, syncInView = false) }
                compose.onNodeWithText("Welcome to Yap").assertIsDisplayed()
                screenshot("desktop-welcome")
                compose.onNodeWithText("Create an account").performClick()
                compose.onNode(hasSetTextAction()).performTextInput("City reader")
                compose.onNodeWithText("Join yap.lgbt").performClick()
                compose.waitUntil(30_000) { controller.welcomeUsername.value == WelcomeUsername.Required }
                compose.onNodeWithText("Enter neighborhood").assertDoesNotExist()
                compose.onNodeWithText("Claim username").assertIsNotEnabled()
                compose.onNode(hasSetTextAction()).performTextInput("City_reader")
                compose.onNodeWithText("Claim username").performClick()
                compose.waitUntil(30_000) { controller.welcomeUsername.value == WelcomeUsername.Confirmed }
                assertEquals("city_reader@neighborhood.test", controller.state.value.privacy.username)
                compose.onNodeWithText("Enter neighborhood").performClick()
                compose.waitUntil(5_000) { !controller.backupStep.value }
                assertFalse(controller.state.value.online)
                val verticalList = SemanticsMatcher.keyIsDefined(SemanticsProperties.VerticalScrollAxisRange)
                compose.onNode(verticalList and hasScrollToIndexAction()).performTouchInput {
                    swipeDown(startY = height * 0.15f, endY = height * 0.8f, durationMillis = 420)
                }
                compose.waitUntil(30_000) {
                    controller.state.value.online && compose.onAllNodes(
                        hasContentDescription("Refresh posts") and isEnabled()
                    ).fetchSemanticsNodes().isNotEmpty()
                }
                for ((screen, request, action) in listOf(
                    Triple("Saved", "feed", "Refresh posts"),
                    Triple("You", "feed", "Refresh profile"),
                    Triple("Groups", "groups", "Refresh groups"),
                    Triple("Inbox", "feed", "Refresh inbox")
                )) {
                    compose.onNodeWithText(screen).performClick()
                    if (screen == "Groups") compose.waitUntil(30_000) { "groups" in calls && !controller.state.value.groupsBusy }
                    calls.clear()
                    compose.onNode(verticalList and hasScrollToIndexAction()).performTouchInput {
                        swipeDown(startY = height * 0.2f, endY = height * 0.85f, durationMillis = 420)
                    }
                    compose.waitUntil(30_000) {
                        request in calls && compose.onAllNodes(
                            hasContentDescription(action) and isEnabled()
                        ).fetchSemanticsNodes().isNotEmpty()
                    }
                }
                compose.onNodeWithText("Sync now").assertDoesNotExist()
                assertTrue(controller.state.value.directAvailable, "Original device registered its private inbox")
                val backup = runBlocking { controller.recoveryKey() }
                DesktopStorage(home.resolve("second")).use { second ->
                    val restored = NetkitController(second, DesktopNetkitEngine()) { Files.readString(publicDocument) }
                    restored.prepareWelcome(false)
                    runBlocking {
                        restored.finishWelcome("Reader", backup)
                        assertTrue(restored.backupStep.value)
                        restored.checkWelcomeUsername()
                        assertEquals(WelcomeUsername.Confirmed, restored.welcomeUsername.value)
                        restored.enterNeighborhood()
                        restored.sync()
                        assertTrue(restored.state.value.online)
                        assertTrue(restored.state.value.mailboxElsewhere)
                        assertFalse(restored.state.value.directAvailable)
                        assertEquals(OTHER_MESSAGING_DEVICE, restored.state.value.messageStatus)
                        assertFailsWith<IllegalStateException> { restored.sendMessage(MessageContact("peer", "Neighbor"), "Hello") }
                        assertEquals(true, restored.backgroundRefresh(NearbyPreference()))
                        assertTrue(restored.recoveryKey() == backup)
                        restored.shutdown()
                    }
                    val reopened = NetkitController(second, DesktopNetkitEngine()) { error("Offline") }
                    reopened.prepareWelcome(false)
                    assertFalse(reopened.welcome.value)
                    assertFalse(reopened.backupStep.value)
                    runBlocking {
                        reopened.restore()
                        assertTrue(reopened.state.value.mailboxElsewhere)
                        assertFalse(reopened.state.value.directAvailable)
                        reopened.shutdown()
                    }
                }
                runBlocking {
                    controller.sync()
                    assertTrue(controller.state.value.directAvailable)
                    assertFalse(controller.state.value.mailboxElsewhere)
                    controller.shutdown()
                }
            }
        } finally { service.destroyForcibly(); service.waitFor() }
    }

    @Test fun profileCropSavesTheVisibleRegion() {
        val surface = org.jetbrains.skia.Surface.makeRasterN32Premul(240, 120)
        try {
            surface.canvas.clear(org.jetbrains.skia.Color.RED)
            org.jetbrains.skia.Paint().use { paint ->
                paint.color = org.jetbrains.skia.Color.BLUE
                surface.canvas.drawRect(org.jetbrains.skia.Rect.makeXYWH(120f, 0f, 120f, 120f), paint)
            }
            surface.makeImageSnapshot().use { image ->
                var saved: String? = null
                compose.setContent { PhotoCropEditor(SkiaCropSource(image), onCancel = {}, onCrop = { saved = it }) }
                val preview = compose.onNodeWithContentDescription("Profile photo crop")
                fun previewPixel(): Int {
                    val bitmap = preview.captureToImage().asSkiaBitmap()
                    return bitmap.getColor(bitmap.width * 3 / 4, bitmap.height / 2)
                }
                assertTrue(org.jetbrains.skia.Color.getB(previewPixel()) > 200)
                compose.onNodeWithContentDescription("Zoom").performSemanticsAction(
                    androidx.compose.ui.semantics.SemanticsActions.SetProgress
                ) { it(2f) }
                preview.performTouchInput { swipeRight() }
                assertTrue(org.jetbrains.skia.Color.getR(previewPixel()) > 200)
                compose.onNodeWithText("Use crop").performClick()
                val rendered = assertNotNull(decodeProfilePhoto(assertNotNull(saved))).asSkiaBitmap()
                assertEquals(PROFILE_PHOTO_SIZE, rendered.height)
                assertTrue(org.jetbrains.skia.Color.getR(rendered.getColor(rendered.width * 3 / 4, rendered.height / 2)) > 200)
                val crop = PhotoFraming(image.width, image.height)
                assertEquals(60f, crop.bounds.left)
                for ((shift, red) in listOf(4f to true, -4f to false)) {
                    crop.adjust(2f, androidx.compose.ui.geometry.Offset(shift, 0f))
                    assertTrue(crop.bounds.left >= 0 && crop.bounds.right <= image.width)
                    val jpeg = encodePhotoCrop(image, crop.bounds)
                    assertTrue(jpeg.length <= PROFILE_PHOTO_LENGTH)
                    val thumbnail = assertNotNull(decodeProfilePhoto(jpeg))
                    assertEquals(PROFILE_PHOTO_SIZE, thumbnail.height)
                    val pixel = thumbnail.asSkiaBitmap().getColor(thumbnail.width / 2, thumbnail.height / 2)
                    assertEquals(red, org.jetbrains.skia.Color.getR(pixel) > org.jetbrains.skia.Color.getB(pixel))
                    val legacy = fitProfileThumbnail(jpeg, LEGACY_PROFILE_PHOTO_LENGTH)
                    assertTrue(legacy.length <= LEGACY_PROFILE_PHOTO_LENGTH)
                    assertEquals(96, assertNotNull(decodeProfilePhoto(legacy)).height)
                    assertEquals(legacy, fitProfileThumbnail(legacy, LEGACY_PROFILE_PHOTO_LENGTH))
                }
            }
        } finally { surface.close() }
    }

    @Test fun desktopNavigationPostingAndComments() {
        val directory = Files.createTempDirectory("yappers-desktop-ui-")
        DesktopStorage(directory).use { storage ->
            val width = mutableStateOf(1024.dp)
            compose.setContent {
                Box(Modifier.width(width.value).fillMaxHeight().testTag("desktop-viewport")) {
                    YappersApp(storage.local, desktop = true)
                }
            }
            compose.waitForIdle()
            compose.onNodeWithText("Your corner.").assertIsDisplayed()
            screenshot("desktop-nearby")
            compose.onNodeWithText("Popular").assertDoesNotExist()
            compose.onNodeWithText("Filter").performClick()
            compose.onNodeWithText("Popular").performClick()
            compose.onNodeWithText("Plans").performClick()
            compose.onNodeWithText("Apply").performClick()
            compose.onNodeWithText("Filter (2)").performClick()
            compose.onNodeWithText("New").performClick()
            compose.onNodeWithText("All").performClick()
            compose.onNodeWithText("Apply").performClick()
            compose.onNodeWithText("Groups", useUnmergedTree = false).performClick()
            compose.onNodeWithText("Discover groups").assertIsDisplayed()
            screenshot("desktop-groups")
            compose.onNodeWithText("You", useUnmergedTree = false).performClick()
            compose.onNodeWithText("Edit profile").performClick()
            compose.onAllNodes(isRoot()).assertCountEquals(1)
            compose.onNodeWithText("Your posts").assertDoesNotExist()
            val pronouns = compose.onNode(hasText("Pronouns") and hasSetTextAction())
            pronouns.performScrollTo()
            pronouns.performTextInput("they / them")
            compose.onAllNodes(isRoot()).assertCountEquals(1)
            compose.onNodeWithText("Edit your profile").assertExists()
            screenshot("desktop-profile-editor")
            compose.onNodeWithText("Save profile").performClick()
            compose.onNodeWithText("they / them").assertIsDisplayed()
            compose.onNodeWithText("Nearby", useUnmergedTree = false).performClick()
            compose.onNodeWithText("Post", useUnmergedTree = false).performClick()
            compose.onNodeWithText("Headline", substring = true).performTextInput("Hello from desktop")
            compose.onAllNodes(hasSetTextAction())[1].performTextInput("A desktop conversation for live testing.")
            compose.onNodeWithText("Post as Anonymous").performClick()
            compose.onNodeWithTag("named-posting-identity").performClick()
            screenshot("desktop-post-editor")
            compose.onNodeWithText("Post to nearby").performClick()
            compose.onNodeWithText("Hello from desktop", substring = true).assertExists().performClick()
            compose.onNodeWithText("Replies (0)").performClick()
            compose.onNode(hasSetTextAction()).assertDoesNotExist()
            compose.onNodeWithText("Write a reply").performClick()
            compose.onNode(hasText("Your comment", substring = true) and hasSetTextAction()).assertIsFocused()
            compose.onNodeWithText("Reply as " + FeedRepository(storage.local).snapshot.alias).performClick()
            compose.onNodeWithText("Anonymous", substring = false).performClick()
            compose.onNodeWithText("Reply as Anonymous").assertIsDisplayed()
            compose.onNodeWithText("Your comment", substring = true).performTextInput("Two lines from desktop.\nA little more room to chat.")
            compose.onNodeWithText("Hide editor").performClick()
            compose.onNode(hasSetTextAction()).assertDoesNotExist()
            compose.onNodeWithText("Continue reply").performClick()
            compose.onNodeWithText("Post comment").performClick()
            compose.onNodeWithText("Two lines from desktop.\nA little more room to chat.").assertIsDisplayed()
            screenshot("desktop-comment-posted")
            compose.runOnIdle { width.value = 540.dp }
            compose.waitForIdle()
            compose.onNode(hasText("Nearby") and SemanticsMatcher.expectValue(SemanticsProperties.Role, Role.Tab)).assertIsDisplayed()
            compose.onNodeWithText("Your corner.").assertDoesNotExist()
            screenshot("desktop-compact")
            compose.onNodeWithContentDescription("Save post").performScrollTo().performClick()
            compose.onNodeWithText("Saved").performClick()
            compose.onNodeWithText("Hello from desktop", substring = true).performClick()
            val savedBack = compose.onNode(hasText("Saved") and SemanticsMatcher.expectValue(SemanticsProperties.Role, Role.Button))
            savedBack.performClick()
            compose.onNodeWithText("Saved posts").assertIsDisplayed()
        }
    }

    @OptIn(ExperimentalTestApi::class)
    @Test fun wideThreadKeepsTheFeedAndAnonymousBylineInView() {
        val directory = Files.createTempDirectory("yappers-desktop-thread-")
        DesktopStorage(directory).use { storage ->
            runSkikoComposeUiTest(size = Size(1280f, 840f), effectContext = motionScale.let { scale ->
                object : MotionDurationScale { override val scaleFactor get() = scale.floatValue }
            }, testTimeout = 30.seconds) {
                val width = mutableStateOf(1280.dp)
                setContent {
                    Box(Modifier.width(width.value).fillMaxHeight()) {
                        YappersApp(storage.local, desktop = true)
                    }
                }
                onNodeWithText("All cities").assertExists()
                onNodeWithContentDescription("View Anonymous neighbor's profile").assertDoesNotExist()
                onNodeWithText("Replies (2)").performClick()
                onNodeWithText("Write a reply").assertExists()
                onNodeWithText("All cities").assertExists()
                saveScreenshot("desktop-thread-beside-feed", onAllNodes(isRoot()).onLast())
                runOnIdle { width.value = 820.dp }
                onNodeWithText("Write a reply").assertExists()
                onNodeWithText("All cities").assertDoesNotExist()
                onNode(hasText("Nearby") and SemanticsMatcher.expectValue(SemanticsProperties.Role, Role.Button)).performClick()
                onNodeWithText("All cities").assertExists()
            }
        }
    }

    @Test fun privacySettingsAtCompactWidthAndLargeText() {
        val dark = mutableStateOf(false)
        val large = mutableStateOf(false)
        val policy = mutableStateOf(DevicePrivacy())
        compose.setContent {
            val density = androidx.compose.ui.platform.LocalDensity.current
            androidx.compose.runtime.CompositionLocalProvider(
                androidx.compose.ui.platform.LocalDensity provides androidx.compose.ui.unit.Density(density.density, if (large.value) 1.6f else 1f)
            ) {
                androidx.compose.material3.MaterialTheme(colorScheme = if (dark.value) DarkColors else LightColors) {
                    androidx.compose.material3.Surface(Modifier.width(390.dp).fillMaxHeight().testTag("privacy-viewport")) {
                        PrivacyScreen(policy.value, null, {}, { update ->
                            policy.value = policy.value.copy(settings = update(policy.value.settings))
                        }, {})
                    }
                }
            }
        }
        compose.onNodeWithText("Hide anonymous content").performClick().assertIsOn()
        compose.onNodeWithText("Minimum account age").performClick()
        compose.onNodeWithText("7 days").performClick()
        compose.onNodeWithText("7 days").assertIsDisplayed()
        screenshot("privacy-light")
        compose.runOnIdle { dark.value = true }
        screenshot("privacy-dark")
        compose.runOnIdle { large.value = true }
        compose.onNodeWithText("Approved people only").performScrollTo().performClick().assertIsOn()
        compose.onNodeWithText("Inbox minimum age").assertDoesNotExist()
        screenshot("privacy-large")
        assertEquals(7,policy.value.settings.feedDays)
    }

    @Test fun nearbyPickerSupportsSearchRadiusAndLocationDenial() {
        val places = object : NearbyServices {
            override fun request(operation: String, query: String, onResult: (String?, String?) -> Unit) {
                if (operation == "locate") onResult(null, "Location permission denied")
                else onResult("""[{"label":"Cambridge, MA, US","lat100":4237,"lon100":-7111}]""", null)
            }
        }
        DesktopStorage(Files.createTempDirectory("yap-nearby-ui-")).use { storage ->
            compose.setContent { YappersApp(storage.local, desktop = true, nearbyServices = places) }
            compose.onNodeWithText("All cities").performClick()
            compose.onNodeWithText("Use my current location").performClick()
            compose.onNodeWithText("Location permission denied").assertExists()
            compose.onNodeWithText("Search cities and towns").performTextInput("Cambridge")
            compose.waitUntil(5000) { compose.onAllNodesWithText("Cambridge, MA, US").fetchSemanticsNodes().isNotEmpty() }
            compose.onNodeWithText("Cambridge, MA, US").performClick()
            compose.onNodeWithText("Radius in miles").performTextReplacement("0")
            compose.onNodeWithText("Apply").assertIsNotEnabled()
            compose.onNodeWithText("Radius in miles").performTextReplacement("12")
            screenshot("nearby-picker")
            compose.onNodeWithText("Apply").performClick()
            compose.onNodeWithText("Within 12 miles").assertIsDisplayed()
            assertEquals(12, FeedRepository(storage.local).snapshot.nearby.radiusMiles)
        }
    }

    @Test fun groupDraftKeepsLocalityChangesWhenDismissalIsCancelled() {
        compose.setContent {
            GroupsScreen(NetkitState(enabled = true), onConnect = {}, onCity = {},
                onOpen = {}, onJoin = {}, onCreate = { _, _, _, _ -> }, onError = { fail(it) })
        }
        compose.onNodeWithText("Create group").performClick()
        compose.onNodeWithText("City").performScrollTo().performTextInput("Boston")
        compose.onNodeWithText("Cancel").performClick()
        compose.onNodeWithText("Keep editing").performClick()
        compose.onNodeWithText("Boston").performScrollTo().assertIsDisplayed()
        compose.onNodeWithText("Cancel").performClick()
        compose.onNodeWithText("Discard draft").performClick()
        compose.onNodeWithText("Discover groups").assertIsDisplayed()
    }

    @Test fun groupDirectoryKeepsWorldwideReadableWithLargeText() {
        compose.setContent {
            val density = androidx.compose.ui.platform.LocalDensity.current
            androidx.compose.runtime.CompositionLocalProvider(
                androidx.compose.ui.platform.LocalDensity provides androidx.compose.ui.unit.Density(density.density, 1.75f)
            ) {
                Box(Modifier.width(390.dp)) {
                    GroupsScreen(NetkitState(enabled = true), onConnect = {}, onCity = {},
                        onOpen = {}, onJoin = {}, onCreate = { _, _, _, _ -> }, onError = { fail(it) })
                }
            }
        }
        val layout = mutableListOf<TextLayoutResult>()
        compose.onNodeWithText("Worldwide", useUnmergedTree = true)
            .performSemanticsAction(SemanticsActions.GetTextLayoutResult) { it(layout) }
        assertEquals(1, layout.single().lineCount)
        compose.onNodeWithText("Worldwide").performClick().assertIsSelected()
    }

    @Test fun portableControllerRecoveryKeepsLocalReceiptsAndResumesMigration() = runBlocking {
        val fixture = System.getenv("YAPPERS_DESKTOP_FIXTURE") ?: return@runBlocking
        val joinA = Files.readString(Path.of(fixture, "join-a.json"))
        val joinB = Files.readString(Path.of(fixture, "join-b.json"))
        DesktopStorage(Files.createTempDirectory("yap-owner-source-")).use { sourceStorage ->
            DesktopStorage(Files.createTempDirectory("yap-owner-destination-")).use { destinationStorage ->
                val source = NetkitController(sourceStorage, DesktopNetkitEngine())
                val destination = NetkitController(destinationStorage, DesktopNetkitEngine())
                try {
                    source.connect(joinA, "Portable Kotlin neighbor")
                    source.publish("Own words, wherever home is.", "Chatter", false, "Portable Kotlin neighbor", "Portable Kotlin ownership")
                    source.sync()
                    assertTrue(source.state.value.online)
                    val post = source.state.value.snapshot.posts.single { it.headline == "Portable Kotlin ownership" }
                    source.reply(post.id, "My comment belongs in my export too."); source.vote(post.id, 1); source.sync()
                    assertEquals(0, source.state.value.pendingCount)
                    source.shutdown() // No active connection: export must work from durable local receipts.
                    val encrypted = source.exportRecovery("a long private Kotlin recovery passphrase", buildJsonObject {
                        put("profile", buildJsonObject { put("pronouns", "they/them"); put("bio", "A portable profile") })
                    })
                    val local = destination.importRecovery(encrypted, "a long private Kotlin recovery passphrase", joinB, "Portable Kotlin neighbor")
                    assertEquals("they/them", local.getValue("client").jsonObject.getValue("profile").jsonObject.getValue("pronouns").jsonPrimitive.content)
                    val firstIdentity = Json.parseToJsonElement(destinationStorage.readSecret("identity-v1")).jsonObject
                    destination.importRecovery(encrypted, "a long private Kotlin recovery passphrase", joinB, "Portable Kotlin neighbor")
                    val resumedIdentity = Json.parseToJsonElement(destinationStorage.readSecret("identity-v1")).jsonObject
                    assertTrue(firstIdentity["secret"] == resumedIdentity["secret"], "A retry must not create another writer key")
                    destination.sync() // Also delivers the signed move to the previous home.
                    assertTrue(destination.state.value.online)
                    val disk = Json.parseToJsonElement(destinationStorage.readState()).jsonObject.getValue("current").jsonObject
                    assertEquals(3, disk.getValue("ownedEvents").jsonArray.size)
                    assertTrue(disk.getValue("accountNotices").jsonArray.isEmpty())
                    source.restore(); source.sync()
                    assertFalse(source.state.value.online)
                    assertTrue(source.state.value.status.contains("moved") || source.state.value.status.contains("revoked"))
                    destination.publish("Same person, new home.", "Chatter", false, "Portable Kotlin neighbor", "Portable Kotlin after move")
                    destination.sync(); assertEquals(0, destination.state.value.pendingCount)
                } finally { source.shutdown(); destination.shutdown() }
            }
        }
    }

    @Test fun portableRecoveryPreservesUncertainUploadsAsExplicitDrafts() = runBlocking {
        val fixture = System.getenv("YAPPERS_DESKTOP_FIXTURE") ?: return@runBlocking
        DesktopStorage(Files.createTempDirectory("yap-draft-source-")).use { sourceStorage ->
            DesktopStorage(Files.createTempDirectory("yap-draft-destination-")).use { targetStorage ->
                val source = NetkitController(sourceStorage, DesktopNetkitEngine())
                val target = NetkitController(targetStorage, DesktopNetkitEngine())
                try {
                    source.connect(Files.readString(Path.of(fixture, "join-a.json")), "Draft owner"); source.sync()
                    source.publish("This has not been uploaded.", "Chatter", false, "Draft owner", "Unsent portable draft")
                    source.shutdown()
                    val encrypted = source.exportRecovery("another long private recovery passphrase")
                    val join = Files.readString(Path.of(fixture, "join-b.json"))
                    assertFailsWith<IllegalStateException> { target.importRecovery(encrypted, "another long private recovery passphrase", join, "Draft owner") }
                    val local = target.importRecovery(encrypted, "another long private recovery passphrase", join, "Draft owner", pendingAsDrafts = true)
                    assertEquals(1, local.getValue("recoveredPendingEvents").jsonArray.size)
                    assertEquals(0, target.state.value.pendingCount)
                    target.sync()
                    assertTrue(target.state.value.snapshot.posts.none { it.headline == "Unsent portable draft" }, "Uncertain drafts must never be silently republished")
                } finally { source.shutdown(); target.shutdown() }
            }
        }
    }

    @Test fun ownershipFormsKeepSecretsMaskedAndRequireExplicitRecoveryChoices(): Unit = runBlocking {
        val files = object : RecoveryFiles {
            override fun saveRecovery(ciphertext: String, onResult: (String?, String?) -> Unit) = error("No real file picker may open in a headless test")
            override fun loadRecovery(onResult: (String?, String?) -> Unit) { onResult("synthetic-encrypted-fixture", null) }
        }
        val engine = object : NetkitEngine {
            override val supportsPortability = true
            override suspend fun portability(command: JsonObject) = buildJsonObject {
                put("wallet", buildJsonObject {
                    put("deviceSecret", "synthetic-device-only")
                    put("nextSequence", 1)
                    put("pending", buildJsonArray {})
                    put("chain", buildJsonArray {
                        add(buildJsonObject {
                            put("payload", buildJsonObject {
                                put("account", "synthetic-account")
                                put("home", "form-home")
                                put("devices", buildJsonArray { add("synthetic-device") })
                            })
                        })
                    })
                })
            }
            override suspend fun identity() = buildJsonObject { put("secret", "isolated-form-fixture") }
            override suspend fun open(config: JsonObject) = buildJsonObject { put("handle", 1); put("home", "form-home"); put("community", "form-community") }
            override suspend fun request(handle: Long, command: JsonObject) = buildJsonObject { put("posts", buildJsonArray {}); put("eventCount", 0) }
            override suspend fun close(handle: Long) {}
        }
        DesktopStorage(Files.createTempDirectory("yap-ownership-form-")).use { storage ->
            val controller = NetkitController(storage, engine)
            try {
                controller.connect("{\"serverAddr\":{\"id\":\"form-home\"}}", "Fixture neighbor")
                compose.setContent { YappersApp(storage.local, controller, initialTab = "You", recoveryFiles = files, desktop = true) }
                compose.onNode(hasScrollToIndexAction()).performScrollToNode(hasText("Backup files"))
                compose.onNodeWithText("Save a backup file").assertDoesNotExist()
                compose.onNodeWithText("Backup files").performScrollTo().performClick()
                compose.onNodeWithText("Save a backup file").performScrollTo().performClick()
                compose.onNodeWithText("Save backup").assertIsNotEnabled()
                compose.onNodeWithText("Backup password").performTextInput("a long synthetic test passphrase")
                compose.onNodeWithText("Save backup").assertIsNotEnabled()
                compose.onNodeWithText("Confirm password").performTextInput("different")
                compose.onNodeWithText("Save backup").assertIsNotEnabled()
                compose.onNodeWithText("Confirm password").performTextReplacement("a long synthetic test passphrase")
                compose.onNodeWithText("Save backup").assertIsEnabled()
                compose.onAllNodes(SemanticsMatcher.keyIsDefined(SemanticsProperties.Password)).assertCountEquals(2)
                screenshot("ownership-export")
                compose.onNodeWithText("Cancel").performClick()
                compose.onNodeWithText("Move to another community").performScrollTo().performClick()
                compose.onNodeWithText("Move account").assertIsNotEnabled()
                compose.onNodeWithText("Choose backup file").performClick()
                compose.onNodeWithText("Backup password").performTextInput("a long synthetic test passphrase")
                compose.onNodeWithText("New community invitation").performTextInput("{\"private\":\"synthetic fixture only\"}")
                compose.onNodeWithText("Move account").assertIsEnabled()
                compose.onNode(hasText("Keep unfinished posts as drafts") and isToggleable()).assertIsOff().performClick().assertIsOn()
                compose.onAllNodes(SemanticsMatcher.keyIsDefined(SemanticsProperties.Password)).assertCountEquals(2)
                screenshot("ownership-import")
                compose.onNodeWithText("Cancel").performClick()
                compose.onNodeWithText("Move to another community").performScrollTo().performClick()
                compose.onNodeWithText("Choose backup file").assertExists()
                compose.onNodeWithText("Move account").assertIsNotEnabled()
                compose.onNode(isToggleable()).assertIsOff()
            } finally { controller.setForeground(false); controller.shutdown() }
        }
    }

    private fun screenshot(name: String) {
        compose.waitForIdle()
        val target = if (name.startsWith("privacy-")) compose.onNodeWithTag("privacy-viewport") else if (name == "desktop-compact") compose.onNodeWithTag("desktop-viewport") else compose.onAllNodes(isRoot()).onLast()
        saveScreenshot(name, target)
    }

    private fun saveScreenshot(name: String, target: SemanticsNodeInteraction) {
        val directory = Path.of("build", "desktop-qa")
        Files.createDirectories(directory)
        val image = org.jetbrains.skia.Image.makeFromBitmap(target.captureToImage().asSkiaBitmap())
        Files.write(directory.resolve("$name.png"), checkNotNull(image.encodeToData()).bytes)
    }

    @Test fun nativeAbiAndStorageRoundTrip() = runBlocking {
        val directory = Files.createTempDirectory("yappers-desktop-storage-")
        val engine = DesktopNetkitEngine()
        val secret = engine.identity().getValue("secret").jsonPrimitive.content
        assertEquals(64, secret.length)
        DesktopStorage(directory).use { storage ->
            storage.writeSecret("identity-v1", secret)
            storage.writeState("{\"outbox\":[]}")
            storage.local.write("local profile")
            assertFails { DesktopStorage(directory) }
        }
        DesktopStorage(directory).use { restored ->
            assertEquals(secret, restored.readSecret("identity-v1"))
            assertEquals("{\"outbox\":[]}", restored.readState())
            assertEquals("local profile", restored.local.read())
            assertEquals("null", restored.readSecret("missing"))
            assertEquals("rw-------", java.nio.file.attribute.PosixFilePermissions.toString(Files.getPosixFilePermissions(directory.resolve("identity-v1.json"))))
        }
        val failure = assertFailsWith<IllegalStateException> {
            engine.request(999999, buildJsonObject { put("type", "feed") })
        }
        assertTrue(failure.message.orEmpty().contains("handle"))
    }

    @Test fun coldRefreshDeliversQueuedEncryptedMessagesAndFetchesTheInbox() = runBlocking {
        val fixture = System.getenv("YAPPERS_DESKTOP_FIXTURE") ?: return@runBlocking
        val join = Files.readString(Path.of(fixture, "join-a.json"))
        DesktopStorage(Files.createTempDirectory("yap-refresh-sender-")).use { source ->
            DesktopStorage(Files.createTempDirectory("yap-refresh-recipient-")).use { target ->
                val sender = NetkitController(source, DesktopNetkitEngine())
                val recipient = NetkitController(target, DesktopNetkitEngine())
                try {
                    recipient.connect(join, "Background recipient")
                    recipient.publish("Contact for the background test", "Friends", false, "Background recipient", "Refresh test")
                    recipient.sync()
                    sender.connect(join, "Background sender")
                    sender.sync()
                    val post = sender.state.value.snapshot.posts.single { it.headline == "Refresh test" }
                    val contact = sender.messageContact(checkNotNull(post.remoteId))
                    sender.sendMessage(contact, "First queued DM")
                    sender.sendMessage(contact, "Second queued DM")
                } finally {
                    sender.shutdown()
                    recipient.shutdown()
                }

                for ((store, outgoing) in listOf(source to true, target to false)) {
                    val cold = NetkitController(store, DesktopNetkitEngine())
                    cold.setForeground(false)
                    assertEquals(true, cold.backgroundRefresh(NearbyPreference()))
                    val messages = cold.state.value.messages
                    assertEquals(listOf("First queued DM", "Second queued DM"), messages.map { it.body })
                    assertTrue(messages.all { it.outgoing == outgoing && it.status == if (outgoing) "sent" else "received" })
                }
            }
        }
    }

    /** Optional two-home fixture supplied by scripts/test-desktop.sh. Never touches user stores. */
    @Test fun desktopIrohFederation() = runBlocking {
        val fixture = System.getenv("YAPPERS_DESKTOP_FIXTURE") ?: return@runBlocking
        val directory = Files.createTempDirectory("yappers-desktop-live-")
        DesktopStorage(directory).use { storage ->
            val controller = NetkitController(storage, DesktopNetkitEngine())
            try {
                controller.connect(Files.readString(Path.of(fixture, "join-a.json")), "Desktop tester")
                controller.publish("From the desktop Kotlin client over real Iroh.", "Chatter", false, "Desktop tester", "Desktop federation smoke")
                controller.sync()
                assertTrue(controller.state.value.online)
                val post = controller.state.value.snapshot.posts.single { it.headline == "Desktop federation smoke" }
                controller.reply(post.id, "Desktop comment over Iroh.")
                controller.vote(post.id, 1)
                controller.sync()
                assertEquals(0, controller.state.value.pendingCount)
            } finally { controller.shutdown() }
        }
    }
}
