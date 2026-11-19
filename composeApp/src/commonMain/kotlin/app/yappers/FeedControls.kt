package app.yappers

import androidx.compose.runtime.Composable
import androidx.compose.runtime.staticCompositionLocalOf
import androidx.compose.ui.Modifier

internal val LocalFeedControl = staticCompositionLocalOf<(@Composable (FormRow, Modifier) -> Unit)?> { null }
