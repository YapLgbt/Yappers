package app.yappers

import androidx.compose.foundation.layout.*
import androidx.compose.material3.*
import androidx.compose.runtime.Composable
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.Shape
import androidx.compose.ui.unit.dp

@Composable
internal fun ActionIcon(name: String) {
    LineIcon(name, null, Modifier.size(ButtonDefaults.IconSize), tint = LocalContentColor.current)
    Spacer(Modifier.width(ButtonDefaults.IconSpacing))
}

@Composable
internal expect fun YapFilterChip(selected: Boolean, onClick: () -> Unit, label: @Composable () -> Unit, enabled: Boolean = true)

@get:Composable
internal expect val emptyStateShape: Shape

@Composable
internal fun EmptyStateEmblem(icon: String) {
    Surface(shape = emptyStateShape, color = MaterialTheme.colorScheme.secondaryContainer) {
        LineIcon(icon, null, Modifier.padding(16.dp).size(32.dp), tint = MaterialTheme.colorScheme.onSecondaryContainer)
    }
}

@Composable
internal expect fun YapTheme(content: @Composable () -> Unit)

@Composable
internal expect fun LineIcon(name: String, description: String?, modifier: Modifier = Modifier.size(24.dp),
                             tint: Color = MaterialTheme.colorScheme.onSurface, filled: Boolean = false)

@Composable
internal expect fun YapButton(onClick: () -> Unit, modifier: Modifier = Modifier, enabled: Boolean = true,
                             contentPadding: PaddingValues = ButtonDefaults.ContentPadding,
                             content: @Composable RowScope.() -> Unit)

@Composable
internal expect fun YapSegments(labels: List<String>, selected: String, onSelect: (String) -> Unit)

@Composable
internal expect fun YapIconToggle(checked: Boolean, onClick: () -> Unit, colors: IconToggleButtonColors, content: @Composable () -> Unit)

@Composable
internal expect fun ProfilePortrait(name: String, profile: CommunityProfile)

@Composable
internal expect fun GroupLoadingIndicator()
