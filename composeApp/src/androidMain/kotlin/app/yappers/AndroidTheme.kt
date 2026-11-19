package app.yappers

import androidx.compose.foundation.Image
import androidx.compose.foundation.background
import androidx.compose.foundation.isSystemInDarkTheme
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.RowScope
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.selection.selectableGroup
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.Button
import androidx.compose.material3.ButtonDefaults
import androidx.compose.material3.ButtonGroupDefaults
import androidx.compose.material3.ExperimentalMaterial3ExpressiveApi
import androidx.compose.material3.FilledTonalButton
import androidx.compose.material3.FilledTonalIconToggleButton
import androidx.compose.material3.FilterChip
import androidx.compose.material3.FilterChipDefaults
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButtonDefaults
import androidx.compose.material3.IconToggleButtonColors
import androidx.compose.material3.LinearWavyProgressIndicator
import androidx.compose.material3.MaterialExpressiveTheme
import androidx.compose.material3.MaterialShapes
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedButton
import androidx.compose.material3.SegmentedButtonDefaults
import androidx.compose.material3.Shapes
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.material3.ToggleButton
import androidx.compose.material3.ToggleButtonDefaults
import androidx.compose.material3.toShape
import androidx.compose.runtime.Composable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.layout.ContentScale
import androidx.compose.ui.res.painterResource
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.dp
import app.yappers.shared.R

@OptIn(ExperimentalMaterial3ExpressiveApi::class)
internal actual val emptyStateShape: androidx.compose.ui.graphics.Shape
    @Composable get() = MaterialShapes.Cookie6Sided.toShape()

@Composable
internal actual fun YapFilterChip(selected: Boolean, onClick: () -> Unit, label: @Composable () -> Unit, enabled: Boolean) {
    val scheme = MaterialTheme.colorScheme
    FilterChip(selected = selected, onClick = onClick, label = label, enabled = enabled,
        shapes = FilterChipDefaults.shapes(), border = null,
        colors = FilterChipDefaults.filterChipColors(
            containerColor = scheme.surfaceContainerHigh,
            selectedContainerColor = scheme.primary, selectedLabelColor = scheme.onPrimary,
            selectedLeadingIconColor = scheme.onPrimary
        ), leadingIcon = if (selected) ({ SegmentedButtonDefaults.ActiveIcon() }) else null)
}

private val light = LightColors.copy(
    surfaceContainerLowest = Color.White, surfaceContainerLow = Color(0xFFF9F2FE),
    surfaceContainer = Color(0xFFF3EAF9), surfaceContainerHighest = Color(0xFFE9DDEF),
    secondaryContainer = Color(0xFFEDDCF5), onSecondaryContainer = Color(0xFF493155),
    outline = Color(0xFF81728D), inverseSurface = Color(0xFF382D41),
    inverseOnSurface = Color(0xFFFAF0FF), inversePrimary = DarkColors.primary
)
private val dark = DarkColors.copy(
    surfaceContainerLowest = Color(0xFF16101C), surfaceContainerLow = Color(0xFF231B2B),
    surfaceContainer = Color(0xFF292031), surfaceContainerHighest = Color(0xFF3A2E44),
    secondaryContainer = Color(0xFF493550), onSecondaryContainer = Color(0xFFF4DFFB),
    outline = Color(0xFFA795B2), inverseSurface = Color(0xFFF0E3F9),
    inverseOnSurface = Color(0xFF372A40), inversePrimary = LightColors.primary
)
private val shapes = Shapes(
    extraSmall = RoundedCornerShape(8.dp), small = RoundedCornerShape(12.dp), medium = RoundedCornerShape(18.dp),
    large = RoundedCornerShape(24.dp), extraLarge = RoundedCornerShape(32.dp)
)

@Composable
internal actual fun YapTheme(content: @Composable () -> Unit) {
    val type = yapTypography()
    MaterialExpressiveTheme(
        colorScheme = if (isSystemInDarkTheme()) dark else light,
        shapes = shapes,
        typography = type.copy(
            headlineMedium = type.headlineMedium.copy(fontWeight = FontWeight.Bold),
            headlineSmall = type.headlineSmall.copy(fontWeight = FontWeight.Bold),
            titleLarge = type.titleLarge.copy(fontWeight = FontWeight.SemiBold),
            labelLarge = type.labelLarge.copy(fontWeight = FontWeight.SemiBold)
        ),
        content = content
    )
}

@Composable
internal actual fun LineIcon(name: String, description: String?, modifier: Modifier, tint: Color, filled: Boolean) {
    val icon = when (name) {
        "privacy" -> R.drawable.ic_shield
        "guide" -> R.drawable.ic_auto_stories
        "document" -> R.drawable.ic_description
        "key" -> R.drawable.ic_key
        "forward" -> R.drawable.ic_chevron_right
        "external" -> R.drawable.ic_open_in_new
        "inbox" -> R.drawable.ic_inbox
        "edit" -> R.drawable.ic_edit
        "filter" -> R.drawable.ic_tune
        "search" -> R.drawable.ic_search
        "plus" -> R.drawable.ic_add
        "up" -> R.drawable.ic_expand_less
        "down" -> R.drawable.ic_expand_more
        "reply" -> R.drawable.ic_chat_bubble
        "refresh" -> R.drawable.ic_refresh
        "back" -> R.drawable.ic_arrow_back
        "bookmark" -> if (filled) R.drawable.nav_bookmark_selected else R.drawable.nav_bookmark
        "groups" -> R.drawable.nav_groups
        "person" -> R.drawable.nav_person
        "pin" -> R.drawable.nav_location_on
        else -> R.drawable.ic_more_vert
    }
    Icon(painterResource(icon), description, modifier, tint)
}


@Composable
internal actual fun YapButton(onClick: () -> Unit, modifier: Modifier, enabled: Boolean, contentPadding: PaddingValues,
                              content: @Composable RowScope.() -> Unit) {
    Button(onClick = onClick, modifier = modifier, enabled = enabled,
        shapes = ButtonDefaults.shapes(), contentPadding = contentPadding, content = content)
}

@Composable
internal actual fun YapOutlinedButton(onClick: () -> Unit, contentPadding: PaddingValues, content: @Composable RowScope.() -> Unit) {
    OutlinedButton(onClick = onClick, shapes = ButtonDefaults.shapes(), contentPadding = contentPadding, content = content)
}

@Composable
internal actual fun YapFilledTonalButton(onClick: () -> Unit, contentPadding: PaddingValues, modifier: Modifier,
                                        content: @Composable RowScope.() -> Unit) {
    FilledTonalButton(onClick = onClick, modifier = modifier, shapes = ButtonDefaults.shapes(), contentPadding = contentPadding, content = content)
}

@Composable
internal actual fun YapCancelButton(onClick: () -> Unit, enabled: Boolean, label: String) {
    TextButton(onClick = onClick, shapes = ButtonDefaults.shapes(), contentPadding = ButtonDefaults.TextButtonContentPadding, enabled = enabled) { Text(label) }
}

@Composable
internal actual fun YapSegments(labels: List<String>, selected: String, onSelect: (String) -> Unit) {
    val scheme = MaterialTheme.colorScheme
    Row(Modifier.fillMaxWidth().selectableGroup(), horizontalArrangement = Arrangement.spacedBy(ButtonGroupDefaults.ConnectedSpaceBetween)) {
        labels.forEachIndexed { index, label ->
            ToggleButton(checked = label == selected, onCheckedChange = { onSelect(label) },
                modifier = Modifier.weight(1f),
                shapes = when (index) {
                    0 -> ButtonGroupDefaults.connectedLeadingButtonShapes()
                    labels.lastIndex -> ButtonGroupDefaults.connectedTrailingButtonShapes()
                    else -> ButtonGroupDefaults.connectedMiddleButtonShapes()
                },
                colors = ToggleButtonDefaults.colors(containerColor = scheme.surfaceContainerHigh,
                    contentColor = scheme.onSurface, checkedContainerColor = scheme.primary, checkedContentColor = scheme.onPrimary),
                contentPadding = PaddingValues(horizontal = 4.dp)
            ) { Text(label) }
        }
    }
}

@Composable
internal actual fun YapIconToggle(checked: Boolean, onClick: () -> Unit, colors: IconToggleButtonColors, content: @Composable () -> Unit) {
    FilledTonalIconToggleButton(checked = checked, onCheckedChange = { onClick() }, colors = colors,
        shapes = IconButtonDefaults.toggleableShapes(), content = content)
}

@OptIn(ExperimentalMaterial3ExpressiveApi::class)
@Composable
internal actual fun ProfilePortrait(name: String, profile: CommunityProfile) {
    Box(Modifier.size(96.dp), contentAlignment = Alignment.Center) {
        Image(painterResource(R.drawable.estuary), null, Modifier.matchParentSize().clip(MaterialShapes.Cookie9Sided.toShape()),
            contentScale = ContentScale.Crop)
        Box(Modifier.size(78.dp).background(MaterialTheme.colorScheme.surface, CircleShape), contentAlignment = Alignment.Center) {
            ProfileAvatar(name, profile, 72.dp)
        }
    }
}

@Composable
internal actual fun GroupLoadingIndicator() {
    LinearWavyProgressIndicator(Modifier.fillMaxWidth())
}
