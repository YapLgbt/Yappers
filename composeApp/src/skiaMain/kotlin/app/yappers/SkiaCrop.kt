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
    override fun encode(bounds: Rect, maxLength: Int) = encodePhotoCrop(image, bounds, maxLength)
    override fun close() = image.close()
}

internal fun encodePhotoCrop(image: Image, bounds: Rect, maxLength: Int = PROFILE_PHOTO_LENGTH): String {
    val byteLimit = maxLength / 4 * 3
    for (size in profilePhotoSizes(maxLength)) {
        val surface = Surface.makeRasterN32Premul(size, size)
        try {
            surface.canvas.clear(org.jetbrains.skia.Color.WHITE)
            surface.canvas.drawImageRect(image, bounds.skia(), org.jetbrains.skia.Rect.makeWH(size.toFloat(), size.toFloat()),
                SamplingMode.MITCHELL, null, true)
            val thumbnail = surface.makeImageSnapshot()
            try {
                for (quality in profilePhotoQualities(size)) {
                    val jpeg = thumbnail.encodeToData(EncodedImageFormat.JPEG, quality) ?: continue
                    try {
                        if (jpeg.size <= byteLimit) return Base64.Default.encode(jpeg.bytes)
                    } finally { jpeg.close() }
                }
            } finally { thumbnail.close() }
        } finally { surface.close() }
    }
    error("Couldn’t save this crop. Try zooming in or choosing another photo.")
}


@androidx.compose.runtime.Composable
internal actual fun AppBackHandler(enabled: Boolean, onBack: () -> Unit) {}
