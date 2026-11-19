@file:OptIn(kotlinx.cinterop.ExperimentalForeignApi::class, androidx.compose.material3.ExperimentalMaterial3Api::class)

package app.yappers

import androidx.compose.foundation.isSystemInDarkTheme
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.text.TextStyle
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.uikit.LocalUIViewController
import androidx.compose.ui.unit.sp
import kotlinx.cinterop.*
import platform.Foundation.NSNotificationCenter
import platform.Foundation.NSOperationQueue
import platform.UIKit.*

/** Supply UIKit's text metrics and semantic colors to the shared reading and onboarding layouts. */
@Composable
internal fun AppleAppearance(content: @Composable () -> Unit) {
    val host = LocalUIViewController.current
    val dark = isSystemInDarkTheme()
    val scale = LocalDensity.current.fontScale
    var highContrast by remember { mutableStateOf(UIAccessibilityDarkerSystemColorsEnabled()) }
    DisposableEffect(Unit) {
        val center = NSNotificationCenter.defaultCenter
        val token = center.addObserverForName(UIAccessibilityDarkerSystemColorsStatusDidChangeNotification,
            null, NSOperationQueue.mainQueue) { highContrast = UIAccessibilityDarkerSystemColorsEnabled() }
        onDispose { center.removeObserver(token) }
    }
    val colors = remember(host, dark, highContrast) {
        val traits = host.traitCollection
        fun resolve(color: UIColor): Color = memScoped {
            val red = alloc<DoubleVar>()
            val green = alloc<DoubleVar>()
            val blue = alloc<DoubleVar>()
            val alpha = alloc<DoubleVar>()
            color.resolvedColorWithTraitCollection(traits).getRed(red.ptr, green.ptr, blue.ptr, alpha.ptr)
            Color(red.value.toFloat(), green.value.toFloat(), blue.value.toFloat(), alpha.value.toFloat())
        }

        val paper = resolve(UIColor.systemBackgroundColor)
        val label = resolve(UIColor.labelColor)
        val secondaryLabel = resolve(UIColor.secondaryLabelColor)
        val inset = resolve(UIColor.secondarySystemBackgroundColor)
        val grouped = resolve(UIColor.systemGroupedBackgroundColor)
        val separator = resolve(UIColor.separatorColor)
        val tint = resolve(host.view.tintColor)
        val base = if (dark) darkColorScheme() else lightColorScheme()
        base.copy(
            background = paper, onBackground = label, surface = paper, onSurface = label,
            surfaceBright = paper, surfaceDim = grouped, surfaceTint = Color.Transparent,
            surfaceContainerLowest = paper, surfaceContainerLow = grouped, surfaceContainer = inset,
            surfaceContainerHigh = inset, surfaceContainerHighest = resolve(UIColor.tertiarySystemBackgroundColor),
            surfaceVariant = inset, onSurfaceVariant = secondaryLabel, outline = separator, outlineVariant = separator,
            primary = tint, onPrimary = paper, primaryContainer = inset, onPrimaryContainer = label,
            secondary = secondaryLabel, onSecondary = paper, secondaryContainer = inset, onSecondaryContainer = label,
            error = resolve(UIColor.systemRedColor), inverseSurface = label, inverseOnSurface = paper,
        )
    }
    val type = remember(scale) {
        // UIFont already includes Dynamic Type. Compose applies fontScale when it draws each sp.
        fun style(name: String?, weight: FontWeight = FontWeight.Normal): TextStyle {
            val font = UIFont.preferredFontForTextStyle(name)
            return TextStyle(fontSize = (font.pointSize / scale).sp, lineHeight = (font.lineHeight / scale).sp,
                fontWeight = weight, letterSpacing = 0.sp)
        }
        val large = style(UIFontTextStyleLargeTitle, FontWeight.Bold)
        val headline = style(UIFontTextStyleHeadline, FontWeight.SemiBold)
        val subhead = style(UIFontTextStyleSubheadline)
        val caption = style(UIFontTextStyleCaption1)
        Typography(
            displayLarge = large, displayMedium = large, displaySmall = large,
            headlineLarge = large, headlineMedium = style(UIFontTextStyleTitle1, FontWeight.Bold),
            headlineSmall = style(UIFontTextStyleTitle2, FontWeight.Bold),
            titleLarge = style(UIFontTextStyleTitle3, FontWeight.SemiBold), titleMedium = headline, titleSmall = subhead,
            bodyLarge = style(UIFontTextStyleBody), bodyMedium = subhead, bodySmall = style(UIFontTextStyleFootnote),
            labelLarge = headline, labelMedium = caption, labelSmall = style(UIFontTextStyleCaption2),
        )
    }
    MaterialTheme(colorScheme = colors, typography = type) {
        CompositionLocalProvider(LocalRippleConfiguration provides null, content = content)
    }
}
