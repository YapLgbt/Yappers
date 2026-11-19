package app.yappers

import androidx.compose.foundation.background
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.selection.selectable
import androidx.compose.foundation.selection.selectableGroup
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp

private val destinations = listOf("Nearby" to "reply", "Groups" to "groups", "Saved" to "bookmark", "You" to "person")

/** Four equal, fully tappable destinations; the safe-area gutter is separate from the controls. */
@Composable
internal fun YappersBottomNavigation(selected: String, onSelect: (String) -> Unit) {
    val colors = MaterialTheme.colorScheme
    Column(Modifier.fillMaxWidth().background(colors.surfaceVariant).navigationBarsPadding()) {
        HorizontalDivider(color = colors.outlineVariant)
        Row(Modifier.fillMaxWidth().height(IntrinsicSize.Min).padding(horizontal = 10.dp).selectableGroup()) {
            destinations.forEach { (label, icon) ->
                val active = selected == label
                val ink = if (active) colors.primary else colors.onSurfaceVariant
                Column(
                    modifier = Modifier.weight(1f).fillMaxHeight().heightIn(min = 60.dp)
                        .selectable(selected = active, role = Role.Tab, onClick = { onSelect(label) })
                        .padding(horizontal = 4.dp, vertical = 5.dp),
                    horizontalAlignment = Alignment.CenterHorizontally
                ) {
                    LineIcon(icon, null, Modifier.size(24.dp), tint = ink, filled = active && icon == "bookmark")
                    Spacer(Modifier.height(3.dp))
                    Text(label, color = ink, fontSize = 12.sp, lineHeight = 16.sp,
                        fontWeight = if (active) FontWeight.Bold else FontWeight.Medium, maxLines = 2)
                    Spacer(Modifier.weight(1f))
                    Spacer(Modifier.height(4.dp))
                    Box(Modifier.size(width = 20.dp, height = 3.dp).clip(RoundedCornerShape(2.dp))
                        .background(if (active) colors.primary else androidx.compose.ui.graphics.Color.Transparent))
                }
            }
        }
    }
}

/** Wider desktop windows keep destinations beside the reading column. */
@Composable
internal fun YappersSidebar(selected: String, onSelect: (String) -> Unit) {
    val colors = MaterialTheme.colorScheme
    Column(Modifier.width(180.dp).fillMaxHeight().background(colors.surfaceVariant)
        .padding(horizontal = 14.dp, vertical = 26.dp).selectableGroup(),
        verticalArrangement = Arrangement.spacedBy(8.dp)) {
        Text("Your corner.", color = colors.onSurface, fontWeight = FontWeight.Bold,
            modifier = Modifier.padding(horizontal = 12.dp, vertical = 14.dp))
        destinations.forEach { (label, icon) ->
            val active = selected == label
            Row(Modifier.fillMaxWidth().clip(RoundedCornerShape(12.dp))
                .background(if (active) colors.primaryContainer else androidx.compose.ui.graphics.Color.Transparent)
                .selectable(active, role = Role.Tab, onClick = { onSelect(label) })
                .padding(horizontal = 12.dp, vertical = 14.dp),
                verticalAlignment = Alignment.CenterVertically) {
                LineIcon(icon, null, Modifier.size(22.dp), tint = if (active) colors.primary else colors.onSurfaceVariant)
                Spacer(Modifier.width(12.dp))
                Text(label, color = if (active) colors.primary else colors.onSurfaceVariant,
                    fontWeight = if (active) FontWeight.Bold else FontWeight.Medium)
            }
        }
    }
}
