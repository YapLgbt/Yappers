package app.yappers

import java.io.File
import javax.swing.JFileChooser
import javax.swing.SwingUtilities
import kotlin.concurrent.thread
import kotlin.io.encoding.Base64
import kotlin.math.roundToInt
import org.jetbrains.skia.EncodedImageFormat
import org.jetbrains.skia.Image
import org.jetbrains.skia.Rect
import org.jetbrains.skia.SamplingMode
import org.jetbrains.skia.Surface

internal class DesktopPhotoLibrary : ProfilePhotoLibrary {
    override fun choosePhoto(onResult: (String?, String?) -> Unit) {
        SwingUtilities.invokeLater {
            val picker = JFileChooser().apply {
                dialogTitle = "Choose profile photo"
                fileFilter = javax.swing.filechooser.FileNameExtensionFilter("Photos", "jpg", "jpeg", "png")
            }
            if (picker.showOpenDialog(null) != JFileChooser.APPROVE_OPTION) { onResult(null, null); return@invokeLater }
            val file = picker.selectedFile
            thread(isDaemon = true) {
                val result = runCatching { preview(file) }
                SwingUtilities.invokeLater { result.fold({ onResult(it, null) }, { onResult(null, it.message ?: "Couldn’t read that photo.") }) }
            }
        }
    }

    private fun preview(file: File): String {
        require(file.length() <= 20_000_000) { "Choose a photo smaller than 20 MB." }
        Image.makeFromEncoded(file.readBytes()).use { source ->
            require(source.width.toLong() * source.height <= 40_000_000) { "Choose a smaller photo." }
            val scale = minOf(1.0, 1536.0 / maxOf(source.width, source.height))
            val width = (source.width * scale).roundToInt().coerceAtLeast(1)
            val height = (source.height * scale).roundToInt().coerceAtLeast(1)
            Surface.makeRasterN32Premul(width, height).use { surface ->
                surface.canvas.clear(org.jetbrains.skia.Color.WHITE)
                surface.canvas.drawImageRect(source, Rect.makeWH(source.width.toFloat(), source.height.toFloat()),
                    Rect.makeWH(width.toFloat(), height.toFloat()), SamplingMode.LINEAR, null, true)
                surface.makeImageSnapshot().use { preview ->
                    val jpeg = checkNotNull(preview.encodeToData(EncodedImageFormat.JPEG, 92))
                    return jpeg.use { Base64.Default.encode(it.bytes) }
                }
            }
        }
    }
}
