package app.yappers

import androidx.compose.foundation.Canvas
import androidx.compose.ui.graphics.drawscope.drawIntoCanvas
import androidx.compose.ui.graphics.skiaCanvas
import org.jetbrains.skia.Paint
import org.jetbrains.skia.RuntimeEffect
import org.jetbrains.skia.RuntimeShaderBuilder
import androidx.compose.runtime.*
import androidx.compose.ui.Modifier

@Composable
internal actual fun FlagCanvas(code: String, phase: Float, modifier: Modifier) {
    val effect = remember(code) { RuntimeEffect.makeForShader(code) }
    val builder = remember(effect) { RuntimeShaderBuilder(effect) }
    val paint = remember { Paint() }
    DisposableEffect(builder) {
        onDispose { paint.close(); builder.close(); effect.close() }
    }
    Canvas(modifier) {
        builder.uniform("extent", size.width, size.height)
        builder.uniform("phase", phase)
        builder.uniform("edition", 4655f)
        val shader = builder.makeShader()
        paint.shader = shader
        shader.close()
        drawIntoCanvas { it.skiaCanvas.drawRect(org.jetbrains.skia.Rect.makeWH(size.width, size.height), paint) }
    }
}

internal actual val supportsFlagAnimation: Boolean get() = true
