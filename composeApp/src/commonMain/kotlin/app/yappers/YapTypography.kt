package app.yappers

import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Typography
import androidx.compose.runtime.Composable
import androidx.compose.runtime.remember
import androidx.compose.ui.text.TextStyle
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.sp
import app.yappers.resources.Res
import app.yappers.resources.geist_regular
import app.yappers.resources.geist_medium
import app.yappers.resources.geist_semibold
import app.yappers.resources.geist_bold
import app.yappers.resources.geist_serif_regular
import org.jetbrains.compose.resources.Font

@Composable
internal fun yapTypography(): Typography {
    val sans = FontFamily(
        Font(Res.font.geist_regular),
        Font(Res.font.geist_medium, FontWeight.Medium),
        Font(Res.font.geist_semibold, FontWeight.SemiBold),
        Font(Res.font.geist_bold, FontWeight.Bold)
    )
    return remember(sans) {
        val defaults = Typography()
        defaults.copy(
            displayLarge = defaults.displayLarge.copy(fontFamily = sans),
            displayMedium = defaults.displayMedium.copy(fontFamily = sans),
            displaySmall = defaults.displaySmall.copy(fontFamily = sans),
            headlineLarge = defaults.headlineLarge.copy(fontFamily = sans),
            headlineMedium = defaults.headlineMedium.copy(fontFamily = sans),
            headlineSmall = defaults.headlineSmall.copy(fontFamily = sans),
            titleLarge = defaults.titleLarge.copy(fontFamily = sans),
            titleMedium = defaults.titleMedium.copy(fontFamily = sans),
            titleSmall = defaults.titleSmall.copy(fontFamily = sans),
            bodyLarge = defaults.bodyLarge.copy(fontFamily = sans, fontSize = 17.sp, lineHeight = 25.sp),
            bodyMedium = defaults.bodyMedium.copy(fontFamily = sans),
            bodySmall = defaults.bodySmall.copy(fontFamily = sans),
            labelLarge = defaults.labelLarge.copy(fontFamily = sans),
            labelMedium = defaults.labelMedium.copy(fontFamily = sans),
            labelSmall = defaults.labelSmall.copy(fontFamily = sans)
        )
    }
}

internal val postBodyStyle: TextStyle
    @Composable get() = MaterialTheme.typography.bodyLarge.copy(
        fontFamily = FontFamily(Font(Res.font.geist_serif_regular)),
        fontSize = 18.sp, lineHeight = 27.sp, letterSpacing = 0.sp
    )

internal val postTitleStyle: TextStyle
    @Composable get() = postBodyStyle.copy(fontSize = 24.sp, lineHeight = 29.sp)
