package app.yappers

import androidx.compose.runtime.*
import androidx.compose.ui.unit.dp
import androidx.compose.ui.window.*
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.runBlocking
import java.awt.Desktop
import java.awt.desktop.AppReopenedListener
import java.awt.event.WindowAdapter
import java.awt.event.WindowEvent
import java.nio.file.Path

/** Run two windows with separate --data-dir paths to exercise independent Iroh identities. */
fun main(args: Array<String>) {
    require(System.getProperty("os.name").startsWith("Mac")) { "This desktop prototype currently supports macOS." }
    val options = args.toList()
    require(options.isEmpty() || options.size == 2 && options[0] == "--data-dir") { "Usage: Yap [--data-dir PATH]" }
    val directory = if (options.isEmpty()) Path.of(System.getProperty("user.home"), "Library", "Application Support", "Yappers Desktop") else Path.of(options[1]).toAbsolutePath()
    val storage = DesktopStorage(directory)
    val nearby = DesktopNearbyServices()
    val controller = NetkitController(storage, DesktopNetkitEngine()) { nearby.perform("enroll") }
    controller.prepareWelcome(storage.local.read() != null)
    try {
        application(exitProcessOnExit = false) {
            val focused = remember { MutableStateFlow(true) }
            var visible by remember { mutableStateOf(true) }
            val windowState = rememberWindowState(width = 1080.dp, height = 820.dp)
            LaunchedEffect(controller) {
                controller.pollWhileOpen({ FeedRepository(storage.local).snapshot }, focused)
            }
            DisposableEffect(Unit) {
                val desktop = Desktop.getDesktop()
                val reopen = AppReopenedListener {
                    visible = true
                    windowState.isMinimized = false
                }
                desktop.addAppEventListener(reopen)
                desktop.setQuitHandler { _, response ->
                    response.cancelQuit()
                    exitApplication()
                }
                onDispose {
                    desktop.removeAppEventListener(reopen)
                    desktop.setQuitHandler(null)
                }
            }
            Window(onCloseRequest = { focused.value = false; visible = false }, visible = visible,
                title = AppBrand.name, state = windowState) {
                val frame = window
                DisposableEffect(frame) {
                    frame.minimumSize = java.awt.Dimension(420, 600)
                    val focus = object : WindowAdapter() {
                        override fun windowGainedFocus(e: WindowEvent) { focused.value = true }
                        override fun windowLostFocus(e: WindowEvent) { focused.value = false }
                    }
                    frame.addWindowFocusListener(focus)
                    onDispose { frame.removeWindowFocusListener(focus) }
                }
                YappersApp(storage.local, controller, photoLibrary = remember { DesktopPhotoLibrary() }, desktop = true,
                    recoveryFiles = remember { DesktopRecoveryFiles() }, nearbyServices = nearby, syncInView = false)
            }
        }
    } finally {
        controller.setForeground(false)
        runBlocking { controller.shutdown() }
        storage.close()
    }
}
