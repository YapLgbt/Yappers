package app.yappers

import androidx.compose.foundation.layout.*
import androidx.compose.material3.Surface
import androidx.compose.foundation.background
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.verticalScroll
import androidx.compose.ui.Alignment
import androidx.compose.material3.MaterialTheme
import androidx.compose.runtime.*
import androidx.compose.ui.Modifier
import androidx.compose.ui.MotionDurationScale
import androidx.compose.ui.unit.dp
import app.yappers.resources.Res
import org.jetbrains.compose.resources.ExperimentalResourceApi

@Composable
internal expect fun platformReducesMotion(): Boolean
internal expect val supportsFlagAnimation: Boolean

@OptIn(ExperimentalResourceApi::class)
@Composable
internal fun EstuaryOnboardingSurface(step: String = "", header: (@Composable () -> Unit)? = null, footer: (@Composable () -> Unit)? = null, content: @Composable ColumnScope.(String) -> Unit) {
    val motionScale = rememberCoroutineScope().coroutineContext[MotionDurationScale]
    val systemReducesMotion = platformReducesMotion() || motionScale?.scaleFactor == 0f
    var phase by remember { mutableFloatStateOf(0f) }
    var source by remember { mutableStateOf<String?>(null) }
    LaunchedEffect(Unit) {
        source = Res.readBytes("files/fabric.glsl").decodeToString()
            .replace("vec2", "float2").replace("vec3", "float3") +
            "\nhalf4 main(float2 p) { return half4(fabric(p), 1.); }"
    }
    Surface(Modifier.fillMaxSize()) {
        Box(Modifier.fillMaxSize(), contentAlignment = Alignment.Center) {
            source?.let { code ->
                LaunchedEffect(systemReducesMotion) {
                    if (systemReducesMotion || !supportsFlagAnimation) return@LaunchedEffect
                    var previous = withFrameMillis { it }
                    while (true) withFrameMillis { now ->
                        val delta = (now - previous).coerceIn(0, 64)
                        phase = (phase + delta * .00038f) % 6.2831855f
                        previous = now
                    }
                }
                FlagCanvas(code, phase, Modifier.matchParentSize())
            }
            Spacer(Modifier.matchParentSize().background(MaterialTheme.colorScheme.surface.copy(alpha = .84f)))
            Column(Modifier.fillMaxSize().safeDrawingPadding().imePadding(), horizontalAlignment = Alignment.CenterHorizontally) {
                if (header != null) Box(Modifier.widthIn(max = 480.dp).fillMaxWidth().padding(horizontal = 20.dp)) { header() }
                Box(Modifier.fillMaxWidth().weight(1f), contentAlignment = Alignment.Center) {
                    val navigation = LocalMaterialNavigation.current
                    val page: @Composable (String) -> Unit = { visibleStep ->
                        Box(Modifier.fillMaxSize(), contentAlignment = Alignment.Center) {
                            Column(Modifier.widthIn(max = 480.dp).fillMaxWidth().verticalScroll(rememberScrollState()).padding(28.dp),
                                verticalArrangement = Arrangement.spacedBy(15.dp)) { content(visibleStep) }
                        }
                    }
                    if (step.isNotEmpty() && navigation != null) navigation.Step(step, page) else page(step)
                }
                if (footer != null) Box(Modifier.widthIn(max = 480.dp).fillMaxWidth().padding(horizontal = 16.dp, vertical = 8.dp)) { footer() }
            }
        }
    }
}

@Composable
internal expect fun FlagCanvas(code: String, phase: Float, modifier: Modifier)
