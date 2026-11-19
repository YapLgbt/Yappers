package app.yappers

import androidx.compose.runtime.*
import com.sun.jna.NativeLibrary
import kotlinx.coroutines.delay

// The desktop client runs on macOS. Read the same AppKit setting as System Settings.
private object MacDisplayOptions {
    private val objc = NativeLibrary.getInstance("objc")
    private val message = objc.getFunction("objc_msgSend")
    private fun selector(name: String) = objc.getFunction("sel_registerName").invokePointer(arrayOf(name))
    private val workspaceClass = objc.getFunction("objc_getClass").invokePointer(arrayOf("NSWorkspace"))
    private val workspace = message.invokePointer(arrayOf(workspaceClass, selector("sharedWorkspace")))
    private val reduceMotion = selector("accessibilityDisplayShouldReduceMotion")

    fun reduced() = (message.invoke(Byte::class.javaPrimitiveType, arrayOf(workspace, reduceMotion)) as Byte) != 0.toByte()
}

@Composable
internal actual fun platformReducesMotion(): Boolean {
    if (!System.getProperty("os.name").startsWith("Mac")) return false
    val reduced by produceState(initialValue = remember { MacDisplayOptions.reduced() }) {
        // Poll only while the print is mounted, including while its frame clock is stopped.
        while (true) {
            value = MacDisplayOptions.reduced()
            delay(500)
        }
    }
    return reduced
}
