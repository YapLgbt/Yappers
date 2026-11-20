package app.yappers

import android.graphics.Bitmap
import android.graphics.BitmapFactory
import android.graphics.RectF
import android.os.Build
import android.provider.Settings
import androidx.annotation.RequiresApi
import androidx.compose.foundation.Canvas
import androidx.compose.foundation.Image
import androidx.compose.runtime.*
import androidx.compose.ui.Modifier
import androidx.compose.ui.geometry.Rect
import androidx.compose.ui.graphics.*
import androidx.compose.ui.graphics.drawscope.DrawScope
import androidx.compose.ui.graphics.drawscope.drawIntoCanvas
import androidx.compose.ui.layout.ContentScale
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.res.painterResource
import app.yappers.shared.R
import java.io.ByteArrayOutputStream
import kotlin.io.encoding.Base64

@Composable
internal actual fun platformReducesMotion(): Boolean =
    Settings.Global.getFloat(LocalContext.current.contentResolver, Settings.Global.ANIMATOR_DURATION_SCALE, 1f) == 0f

@Composable
internal actual fun FlagCanvas(code: String, phase: Float, modifier: Modifier) {
    if (Build.VERSION.SDK_INT >= 33) AnimatedFlag(code, phase, modifier)
    else Image(painterResource(R.drawable.estuary), null, modifier, contentScale = ContentScale.Crop)
}

@RequiresApi(33)
@Composable
private fun AnimatedFlag(code: String, phase: Float, modifier: Modifier) {
    val shader = remember(code) { android.graphics.RuntimeShader(code) }
    val brush = remember(shader) { ShaderBrush(shader) }
    Canvas(modifier) {
        shader.setFloatUniform("extent", size.width, size.height)
        shader.setFloatUniform("phase", phase)
        shader.setFloatUniform("edition", 4655f)
        drawRect(brush)
    }
}

actual fun decodeProfilePhoto(value: String): ImageBitmap? = runCatching {
    require(value.length <= PROFILE_PHOTO_LENGTH)
    boundedBitmap(Base64.Default.decode(value), PROFILE_PHOTO_SIZE).asImageBitmap()
}.getOrNull()

private fun boundedBitmap(bytes: ByteArray, max: Int): Bitmap {
    val options = BitmapFactory.Options().apply { inJustDecodeBounds = true }
    BitmapFactory.decodeByteArray(bytes, 0, bytes.size, options)
    require(options.outWidth in 1..max && options.outHeight in 1..max)
    return checkNotNull(BitmapFactory.decodeByteArray(bytes, 0, bytes.size))
}

internal actual fun decodeCropSource(bytes: ByteArray): CropSource = object : CropSource {
    private val bitmap = boundedBitmap(bytes, 1536)
    private val paint = android.graphics.Paint(android.graphics.Paint.FILTER_BITMAP_FLAG)
    override val width get() = bitmap.width
    override val height get() = bitmap.height
    private fun paint(canvas: android.graphics.Canvas, bounds: Rect, destination: RectF) {
        val transform = android.graphics.Matrix()
        transform.setRectToRect(RectF(bounds.left, bounds.top, bounds.right, bounds.bottom), destination, android.graphics.Matrix.ScaleToFit.FILL)
        canvas.drawBitmap(bitmap, transform, paint)
    }
    override fun draw(scope: DrawScope, bounds: Rect) = with(scope) {
        drawIntoCanvas { paint(it.nativeCanvas, bounds, RectF(0f, 0f, size.width, size.height)) }
    }
    override fun encode(bounds: Rect, maxLength: Int): String {
        val byteLimit = maxLength / 4 * 3
        for (size in profilePhotoSizes(maxLength)) {
            val thumb = Bitmap.createBitmap(size, size, Bitmap.Config.ARGB_8888)
            try {
                val canvas = android.graphics.Canvas(thumb)
                canvas.drawColor(android.graphics.Color.WHITE)
                paint(canvas, bounds, RectF(0f, 0f, size.toFloat(), size.toFloat()))
                val out = ByteArrayOutputStream()
                for (quality in profilePhotoQualities(size)) {
                    if (thumb.compress(Bitmap.CompressFormat.JPEG, quality, out) && out.size() <= byteLimit)
                        return Base64.Default.encode(out.toByteArray())
                    out.reset()
                }
            } finally { thumb.recycle() }
        }
        error("Couldn’t save this crop. Choose another photo.")
    }
    override fun close() = bitmap.recycle()
}

internal actual val supportsFlagAnimation: Boolean get() = Build.VERSION.SDK_INT >= 33
