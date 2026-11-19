package app.yappers

import androidx.compose.foundation.Canvas
import androidx.compose.foundation.layout.size
import androidx.compose.material3.LocalContentColor
import androidx.compose.runtime.Composable
import androidx.compose.ui.Modifier
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.semantics.text
import androidx.compose.ui.text.AnnotatedString
import androidx.compose.ui.text.TextStyle
import androidx.compose.ui.text.drawText
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.text.rememberTextMeasurer
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import app.yappers.resources.Res
import app.yappers.resources.leckerli_one_regular
import org.jetbrains.compose.resources.Font
import kotlin.math.round

@Composable
internal fun YapWordmark(modifier: Modifier = Modifier, prominent: Boolean = false) {
    val scale = if (prominent) 1.8f else 1f
    val fontSize = with(LocalDensity.current) { (40.dp * scale).toSp() }
    val layout = rememberTextMeasurer().measure(
        text = AppBrand.wordmark,
        style = TextStyle(
            fontFamily = FontFamily(Font(Res.font.leckerli_one_regular)),
            fontSize = fontSize,
            letterSpacing = 0.sp
        ),
        maxLines = 1,
        softWrap = false
    )
    val ink = LocalContentColor.current
    Canvas(modifier.size(84.dp * scale, 44.dp * scale).semantics { text = AnnotatedString(AppBrand.wordmark) }) {
        // The filled Leckerli One outlines of “yap” balance at (0.7972, -0.1595) em from the baseline origin.
        val em = fontSize.toPx()
        val origin = Offset(
            round(center.x - 0.7972f * em),
            round(center.y - layout.firstBaseline + 0.1595f * em)
        )
        drawText(layout, color = ink, topLeft = origin)
    }
}
