package app.yappers

import app.yappers.resources.Res
import kotlinx.coroutines.runBlocking
import org.jetbrains.compose.resources.ExperimentalResourceApi
import org.jetbrains.skia.Bitmap
import org.jetbrains.skia.Canvas
import org.jetbrains.skia.Paint
import org.jetbrains.skia.Rect
import org.jetbrains.skia.RuntimeEffect
import org.jetbrains.skia.RuntimeShaderBuilder
import kotlin.test.Test
import kotlin.test.assertTrue

class EstuaryPaletteTest {
    @OptIn(ExperimentalResourceApi::class)
    @Test fun rainbowBandsRemainVisibleInGalleryAndBanner() = runBlocking {
        val source = Res.readBytes("files/fabric.glsl").decodeToString()
            .replace("vec2", "float2").replace("vec3", "float3")
        val inks = listOf(0xe40303, 0xff8c00, 0xffed00, 0x008026, 0x004dff, 0x750787)
        RuntimeEffect.makeForShader("$source\nhalf4 main(float2 p) { return half4(finishedPrint(p), 1.); }").use { effect ->
            RuntimeShaderBuilder(effect).use { builder ->
                builder.uniform("edition", 4655f)
                builder.uniform("finishMode", 0f)
                for ((width, height) in listOf(480 to 124, 250 to 300)) {
                    builder.uniform("extent", width.toFloat(), height.toFloat())
                    for (phase in listOf(0f, 2f, 4f)) {
                        builder.uniform("phase", phase)
                        Bitmap().use { bitmap ->
                            bitmap.allocN32Pixels(width, height)
                            Canvas(bitmap).use { canvas ->
                                builder.makeShader().use { shader ->
                                    Paint().use { paint ->
                                        paint.shader = shader
                                        canvas.drawRect(Rect.makeWH(width.toFloat(), height.toFloat()), paint)
                                    }
                                }
                            }
                            val counts = IntArray(inks.size)
                            for (y in 0 until height step 4) for (x in 0 until width step 4) {
                                val pixel = bitmap.getColor(x, y)
                                inks.forEachIndexed { i, ink ->
                                    // Allow the water's neutral shading and fine contour marks.
                                    val distance = listOf(16, 8, 0).sumOf { shift ->
                                        val delta = ((pixel shr shift) and 255) - ((ink shr shift) and 255)
                                        delta * delta
                                    }
                                    if (distance < 55 * 55) counts[i]++
                                }
                            }
                            assertTrue(counts.all { it > width * height / 16 * .02 },
                                "Missing rainbow band at ${width}x$height, phase $phase: ${counts.toList()}")
                        }
                    }
                }
            }
        }
    }
}
