package app.yappers

import androidx.compose.foundation.Canvas
import androidx.compose.foundation.isSystemInDarkTheme
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.RowScope
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.material3.Button
import androidx.compose.material3.FilledTonalIconToggleButton
import androidx.compose.material3.IconToggleButtonColors
import androidx.compose.material3.LinearProgressIndicator
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.SegmentedButton
import androidx.compose.material3.SegmentedButtonDefaults
import androidx.compose.material3.SingleChoiceSegmentedButtonRow
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.Path
import androidx.compose.ui.graphics.drawscope.Stroke
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.unit.dp

internal actual val emptyStateShape: androidx.compose.ui.graphics.Shape
    @Composable get() = androidx.compose.foundation.shape.CircleShape

@Composable
internal actual fun YapFilterChip(selected: Boolean, onClick: () -> Unit, label: @Composable () -> Unit, enabled: Boolean) {
    androidx.compose.material3.FilterChip(selected = selected, onClick = onClick, label = label, enabled = enabled)
}

@Composable
internal actual fun YapTheme(content: @Composable () -> Unit) {
    MaterialTheme(colorScheme = if (isSystemInDarkTheme()) DarkColors else LightColors,
        typography = yapTypography(), content = content)
}

@Composable
internal actual fun LineIcon(name: String, description: String?, modifier: Modifier, tint: Color, filled: Boolean) {
    Canvas(modifier.then(if (description != null) Modifier.semantics { contentDescription = description } else Modifier)) {
        val s = size.minDimension / 24f
        fun path(vararg points: Pair<Float, Float>, closed: Boolean = false) {
            val p = Path(); points.forEachIndexed { i, (x, y) -> if (i == 0) p.moveTo(x*s, y*s) else p.lineTo(x*s, y*s) }
            if (closed) p.close()
            drawPath(p, tint, style = if (filled && closed) androidx.compose.ui.graphics.drawscope.Fill else Stroke(1.8f*s))
        }
        when (name) {
            "refresh" -> {
                drawArc(tint, 32f, 284f, false, androidx.compose.ui.geometry.Offset(4f*s, 4f*s), androidx.compose.ui.geometry.Size(16f*s, 16f*s), style = Stroke(1.8f*s))
                path(19f to 3f, 19f to 8f, 14f to 8f)
            }
            "plus" -> { path(12f to 4f, 12f to 20f); path(4f to 12f, 20f to 12f) }
            "up" -> path(6f to 15f, 12f to 9f, 18f to 15f)
            "down" -> path(6f to 9f, 12f to 15f, 18f to 9f)
            "back" -> path(15f to 5f, 8f to 12f, 15f to 19f)
            "bookmark" -> path(6f to 3f, 18f to 3f, 18f to 21f, 12f to 17f, 6f to 21f, closed = true)
            "reply" -> path(4f to 4f, 20f to 4f, 20f to 16f, 11f to 16f, 5f to 21f, 5f to 16f, 4f to 16f, closed = true)
            "send" -> { path(3f to 3f, 21f to 12f, 3f to 21f, 7f to 12f, closed = true); path(7f to 12f, 21f to 12f) }
            "feed" -> { path(4f to 4f, 20f to 4f, 20f to 20f, 4f to 20f, closed = true); path(8f to 9f, 16f to 9f); path(8f to 14f, 14f to 14f) }
            "groups" -> {
                drawCircle(tint, 3f*s, androidx.compose.ui.geometry.Offset(9f*s, 7f*s), style = Stroke(1.8f*s))
                drawCircle(tint, 2.5f*s, androidx.compose.ui.geometry.Offset(17f*s, 9f*s), style = Stroke(1.8f*s))
                drawArc(tint, 180f, 180f, false, androidx.compose.ui.geometry.Offset(2f*s, 13f*s), androidx.compose.ui.geometry.Size(14f*s, 14f*s), style = Stroke(1.8f*s))
                path(16f to 15f, 20f to 17f, 21f to 20f)
            }
            "person" -> { drawCircle(tint, 3.5f*s, androidx.compose.ui.geometry.Offset(12f*s, 7f*s), style = Stroke(1.8f*s));
                drawArc(tint, 180f, 180f, false, androidx.compose.ui.geometry.Offset(4f*s, 13f*s), androidx.compose.ui.geometry.Size(16f*s, 14f*s), style = Stroke(1.8f*s)) }
            "pin" -> { drawCircle(tint, 7f*s, androidx.compose.ui.geometry.Offset(12f*s, 10f*s), style = Stroke(1.8f*s)); path(7f to 16f, 12f to 22f, 17f to 16f); drawCircle(tint, 2f*s, androidx.compose.ui.geometry.Offset(12f*s, 10f*s)) }
            "spark" -> path(12f to 2f, 15f to 9f, 22f to 12f, 15f to 15f, 12f to 22f, 9f to 15f, 2f to 12f, 9f to 9f, closed = true)
            else -> listOf(5f, 12f, 19f).forEach { x -> drawCircle(tint, 1.5f*s, androidx.compose.ui.geometry.Offset(x*s, 12f*s)) }
        }
    }
}


@Composable
internal actual fun YapButton(onClick: () -> Unit, modifier: Modifier, enabled: Boolean, contentPadding: PaddingValues,
                              content: @Composable RowScope.() -> Unit) {
    Button(onClick = onClick, modifier = modifier, enabled = enabled,
        contentPadding = contentPadding, content = content)
}

@Composable
internal actual fun YapSegments(labels: List<String>, selected: String, onSelect: (String) -> Unit) {
    SingleChoiceSegmentedButtonRow(Modifier.fillMaxWidth()) {
        labels.forEachIndexed { index, label ->
            SegmentedButton(selected = label == selected, onClick = { onSelect(label) },
                shape = SegmentedButtonDefaults.itemShape(index, labels.size)) { Text(label) }
        }
    }
}

@Composable
internal actual fun YapIconToggle(checked: Boolean, onClick: () -> Unit, colors: IconToggleButtonColors, content: @Composable () -> Unit) {
    FilledTonalIconToggleButton(checked = checked, onCheckedChange = { onClick() }, colors = colors,
        content = content)
}

@Composable
internal actual fun ProfilePortrait(name: String, profile: CommunityProfile) = ProfileAvatar(name, profile, 80.dp)

@Composable
internal actual fun GroupLoadingIndicator() = LinearProgressIndicator(Modifier.fillMaxWidth())
