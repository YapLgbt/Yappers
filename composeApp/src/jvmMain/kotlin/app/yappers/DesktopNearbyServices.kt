package app.yappers

import java.io.File
import java.util.concurrent.TimeUnit
import kotlin.concurrent.thread

internal class DesktopNearbyServices : NearbyServices {
    override fun request(operation: String, query: String, onResult: (String?, String?) -> Unit) {
        thread(name = "Yap nearby", isDaemon = true) {
            try {
                val resources = checkNotNull(System.getProperty("compose.application.resources.dir"))
                val helper = File(resources, "YapNearby.app/Contents/MacOS/YapNearby")
                val process = ProcessBuilder(helper.absolutePath, operation, query).start()
                // Drain stdout concurrently; an enrollment or search response can exceed a pipe buffer.
                val deadline = thread(isDaemon = true) {
                    if (!process.waitFor(50, TimeUnit.SECONDS)) process.destroyForcibly()
                }
                val output = process.inputStream.bufferedReader().use { it.readText() }
                val error = process.errorStream.bufferedReader().use { it.readText() }
                deadline.join()
                if (process.exitValue() == 0) onResult(output, null)
                else onResult(null, error.ifBlank { "Location service timed out. Try a city search." })
            } catch (_: Exception) {
                onResult(null, "Couldn’t start the Yap location service.")
            }
        }
    }
}
