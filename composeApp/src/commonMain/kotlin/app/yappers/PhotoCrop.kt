package app.yappers

import androidx.compose.foundation.Canvas
import androidx.compose.foundation.background
import androidx.compose.foundation.gestures.detectTransformGestures
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.geometry.Rect
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.input.pointer.pointerInput
import androidx.compose.ui.semantics.*
import androidx.compose.ui.unit.dp
import kotlin.io.encoding.Base64

internal class PhotoFraming(val width: Int, val height: Int) {
    var zoom by mutableFloatStateOf(1f)
        private set
    private var center by mutableStateOf(Offset(width / 2f, height / 2f))
    private val edge get() = minOf(width, height) / zoom
    val bounds: Rect get() = Rect(center.x - edge / 2, center.y - edge / 2, center.x + edge / 2, center.y + edge / 2)

    // Pan is measured in fractions of the visible crop. Anchor is relative to its center.
    fun adjust(scale: Float, pan: Offset = Offset.Zero, anchor: Offset = Offset.Zero) {
        val previous = edge
        zoom = scale.coerceIn(1f, 4f)
        val half = edge / 2
        val next = center + anchor * (previous - edge) - pan * edge
        center = Offset(next.x.coerceIn(half, width - half), next.y.coerceIn(half, height - half))
    }
}

internal interface CropSource {
    val width: Int
    val height: Int
    fun draw(scope: androidx.compose.ui.graphics.drawscope.DrawScope, bounds: Rect)
    fun encode(bounds: Rect, maxLength: Int = PROFILE_PHOTO_LENGTH): String
    fun close()
}
internal expect fun decodeCropSource(bytes: ByteArray): CropSource

internal fun profilePhotoSizes(maxLength: Int): List<Int> = when {
    maxLength > PROFILE_THUMBNAIL_LENGTH -> listOf(PROFILE_PHOTO_SIZE)
    maxLength > LEGACY_PROFILE_PHOTO_LENGTH -> listOf(PROFILE_THUMBNAIL_SIZE, 320, 256, 192, 128, 96)
    else -> listOf(96)
}

internal fun profilePhotoQualities(size: Int): List<Int> = when (size) {
    PROFILE_PHOTO_SIZE -> listOf(87, 78, 68, 58, 48, 38, 28, 18, 8)
    96 -> listOf(82, 65, 45, 30, 15, 5)
    else -> listOf(82, 65, 50, 40)
}

internal fun fitProfileThumbnail(encoded: String, maxLength: Int): String {
    val source = runCatching { decodeCropSource(Base64.Default.decode(encoded)) }.getOrElse {
        if (encoded.length <= maxLength) return encoded
        throw it
    }
    try {
        val maxSize = if (maxLength <= LEGACY_PROFILE_PHOTO_LENGTH) 128 else PROFILE_THUMBNAIL_SIZE
        if (encoded.length <= maxLength && maxOf(source.width, source.height) <= maxSize) return encoded
        val side = minOf(source.width, source.height).toFloat()
        val left = (source.width - side) / 2
        val top = (source.height - side) / 2
        return source.encode(Rect(left, top, left + side, top + side), maxLength)
    } finally { source.close() }
}

@Composable
internal fun PhotoCropEditor(image: CropSource, onCancel: () -> Unit, onCrop: (String) -> Unit) {
    val framing = remember(image) { PhotoFraming(image.width, image.height) }
    var error by remember(image) { mutableStateOf<String?>(null) }
    Column(Modifier.fillMaxSize().padding(horizontal = 22.dp), horizontalAlignment = Alignment.CenterHorizontally) {
        Row(Modifier.fillMaxWidth().padding(vertical = 12.dp), horizontalArrangement = Arrangement.SpaceBetween) {
            YapCancelButton(onClick = onCancel)
            YapButton(onClick = {
                runCatching { image.encode(framing.bounds) }.onSuccess(onCrop).onFailure { error = it.message }
            }) { Text("Use crop") }
        }
        Column(Modifier.weight(1f).verticalScroll(rememberScrollState()).widthIn(max = 360.dp).padding(vertical = 12.dp),
            horizontalAlignment = Alignment.CenterHorizontally, verticalArrangement = Arrangement.spacedBy(12.dp)) {
            Text("Crop photo", style = MaterialTheme.typography.headlineMedium)
            Canvas(Modifier.widthIn(max = 288.dp).fillMaxWidth().aspectRatio(1f).clip(CircleShape)
                .background(androidx.compose.ui.graphics.Color.White).pointerInput(image) {
                    detectTransformGestures { centroid, pan, scale, _ ->
                        val side = size.width.toFloat()
                        framing.adjust(framing.zoom * scale, pan / side, centroid / side - Offset(.5f, .5f))
                    }
                }.semantics {
                    contentDescription = "Profile photo crop"
                    customActions = listOf(
                        CustomAccessibilityAction("Move photo left") { framing.adjust(framing.zoom, Offset(-.1f, 0f)); true },
                        CustomAccessibilityAction("Move photo right") { framing.adjust(framing.zoom, Offset(.1f, 0f)); true },
                        CustomAccessibilityAction("Move photo up") { framing.adjust(framing.zoom, Offset(0f, -.1f)); true },
                        CustomAccessibilityAction("Move photo down") { framing.adjust(framing.zoom, Offset(0f, .1f)); true }
                    )
                }) {
                image.draw(this, framing.bounds)
            }
            Text("Drag to move your photo.", style = MaterialTheme.typography.bodyMedium)
            Slider(value = framing.zoom, onValueChange = { framing.adjust(it) }, valueRange = 1f..4f,
                modifier = Modifier.fillMaxWidth().semantics { contentDescription = "Zoom" })
            error?.let { Text(it, color = MaterialTheme.colorScheme.error) }
        }
    }
}

class NativePhotoCrop internal constructor(
    val preview: String, private val source: CropSource, private val onCancel: () -> Unit, private val onCrop: (String) -> Unit,
) {
    val width = source.width
    val height = source.height
    fun cancel() = onCancel()

    /** Bounds are in the oriented preview's pixel coordinates, exactly as used for drawing. */
    fun useCrop(left: Double, top: Double, side: Double): String? {
        val edge = side.toFloat().coerceIn(1f, minOf(width, height).toFloat())
        val x = left.toFloat().coerceIn(0f, width - edge)
        val y = top.toFloat().coerceIn(0f, height - edge)
        return runCatching { source.encode(Rect(x, y, x + edge, y + edge)) }
            .fold({ onCrop(it); null }, { it.message ?: "Couldn’t save this crop. Try another image." })
    }
}
internal val LocalNativeCrop = staticCompositionLocalOf<(@Composable (NativePhotoCrop) -> Unit)?> { null }
