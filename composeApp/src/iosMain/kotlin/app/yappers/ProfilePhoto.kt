package app.yappers

import androidx.compose.ui.graphics.ImageBitmap
import androidx.compose.ui.graphics.toComposeImageBitmap
import org.jetbrains.skia.Image
import kotlin.io.encoding.Base64

actual fun decodeProfilePhoto(value: String): ImageBitmap? = runCatching {
    require(value.length <= PROFILE_PHOTO_LENGTH)
    val image = Image.makeFromEncoded(Base64.Default.decode(value))
    require(image.width <= PROFILE_PHOTO_SIZE && image.height <= PROFILE_PHOTO_SIZE)
    image.toComposeImageBitmap()
}.getOrNull()
