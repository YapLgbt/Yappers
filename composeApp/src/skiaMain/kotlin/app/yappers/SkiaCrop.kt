package app.yappers

import androidx.compose.ui.geometry.Rect
import androidx.compose.ui.graphics.drawscope.DrawScope
import androidx.compose.ui.graphics.drawscope.drawIntoCanvas
import androidx.compose.ui.graphics.skiaCanvas
import org.jetbrains.skia.*
import kotlin.io.encoding.Base64

private fun Rect.skia() = org.jetbrains.skia.Rect.makeLTRB(left, top, right, bottom)

internal actual fun decodeCropSource(bytes: ByteArray): CropSource = SkiaCropSource(Image.makeFromEncoded(bytes))

internal class SkiaCropSource(private val image: Image) : CropSource {
    override val width get() = image.width
    override val height get() = image.height
    override fun draw(scope: DrawScope, bounds: Rect) = with(scope) {
        drawIntoCanvas { it.skiaCanvas.drawImageRect(image, bounds.skia(), org.jetbrains.skia.Rect.makeWH(size.width, size.height), SamplingMode.MITCHELL, null, true) }
    }
    override fun encode(bounds: Rect) = encodePhotoCrop(image, bounds)
    override fun close() = image.close()
}

internal fun encodePhotoCrop(image: Image, bounds: Rect): String {
    val surface = Surface.makeRasterN32Premul(96, 96)
    try {
        surface.canvas.clear(org.jetbrains.skia.Color.WHITE)
        surface.canvas.drawImageRect(image, bounds.skia(), org.jetbrains.skia.Rect.makeWH(96f, 96f), SamplingMode.MITCHELL, null, true)
        val thumbnail = surface.makeImageSnapshot()
        try {
            for (quality in listOf(82, 65, 45, 30, 15, 5)) {
                val jpeg = thumbnail.encodeToData(EncodedImageFormat.JPEG, quality) ?: continue
                try {
                    if (jpeg.size <= 3072) return Base64.Default.encode(jpeg.bytes)
                } finally { jpeg.close() }
            }
        } finally { thumbnail.close() }
    } finally { surface.close() }
    error("Couldn’t save this crop. Try zooming in or choosing another photo.")
}


@androidx.compose.runtime.Composable
internal actual fun AppBackHandler(enabled: Boolean, onBack: () -> Unit) {}
