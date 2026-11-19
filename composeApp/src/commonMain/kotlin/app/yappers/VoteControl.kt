package app.yappers

import androidx.compose.foundation.background
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.material3.*
import androidx.compose.runtime.Composable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.text.rememberTextMeasurer
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.unit.dp

@Composable
internal fun VoteControl(score: Int, vote: Int, onVote: (Int) -> Unit) {
    val colors = MaterialTheme.colorScheme
    val type = MaterialTheme.typography.labelLarge.copy(fontFeatureSettings = "tnum")
    // Reserve room for every score this voter's three states can produce, including a minus sign.
    val neutral = score.toLong() - vote
    val measurer = rememberTextMeasurer()
    val width = maxOf(measurer.measure((neutral - 1).toString(), type).size.width,
        measurer.measure((neutral + 1).toString(), type).size.width)
    val scoreWidth = with(LocalDensity.current) { width.toDp().coerceAtLeast(28.dp) }
    Row(Modifier.background(colors.surfaceContainer, CircleShape), verticalAlignment = Alignment.CenterVertically) {
        VoteArrow("up", "Upvote", vote == 1) { onVote(1) }
        Text(score.toString(), Modifier.width(scoreWidth), style = type, color = colors.onSurface,
            textAlign = TextAlign.Center, maxLines = 1, softWrap = false)
        VoteArrow("down", "Downvote", vote == -1) { onVote(-1) }
    }
}

@Composable
private fun VoteArrow(icon: String, label: String, checked: Boolean, onClick: () -> Unit) {
    val colors = MaterialTheme.colorScheme
    Box(Modifier.size(48.dp), contentAlignment = Alignment.Center) {
        YapIconToggle(checked = checked, onClick = onClick,
            colors = IconButtonDefaults.filledTonalIconToggleButtonColors(
                containerColor = colors.surfaceContainer, contentColor = colors.onSurfaceVariant,
                checkedContainerColor = colors.primaryContainer, checkedContentColor = colors.onPrimaryContainer
            )) {
            LineIcon(icon, label, tint = LocalContentColor.current)
        }
    }
}
