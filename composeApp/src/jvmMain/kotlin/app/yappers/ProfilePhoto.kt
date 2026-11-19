package app.yappers

import androidx.compose.ui.graphics.ImageBitmap
import androidx.compose.ui.graphics.toComposeImageBitmap
import org.jetbrains.skia.Image
import kotlin.io.encoding.Base64

actual fun decodeProfilePhoto(value: String): ImageBitmap? = runCatching {
    require(value.length <= 4096)
    val image = Image.makeFromEncoded(Base64.Default.decode(value))
    require(image.width <= 128 && image.height <= 128)
    image.toComposeImageBitmap()
}.getOrNull()
