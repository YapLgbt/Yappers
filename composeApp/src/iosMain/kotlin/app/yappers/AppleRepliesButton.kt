@file:OptIn(kotlinx.cinterop.ExperimentalForeignApi::class)

package app.yappers

import androidx.compose.foundation.layout.size
import androidx.compose.material3.MaterialTheme
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberUpdatedState
import androidx.compose.runtime.setValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.unit.DpSize
import androidx.compose.ui.unit.dp
import androidx.compose.ui.viewinterop.UIKitInteropProperties
import androidx.compose.ui.viewinterop.UIKitView
import kotlinx.cinterop.useContents
import platform.UIKit.NSDirectionalEdgeInsetsMake
import platform.UIKit.UIAction
import platform.UIKit.UIButton
import platform.UIKit.UIButtonConfiguration
import platform.UIKit.UIButtonConfigurationSize
import platform.UIKit.UIColor
import platform.UIKit.UIView
import platform.UIKit.UIViewAutoresizingFlexibleHeight
import platform.UIKit.UIViewAutoresizingFlexibleWidth
import platform.UIKit.UIImage
import platform.UIKit.accessibilityLabel
import platform.UIKit.secondaryLabelColor
import kotlin.math.ceil

// UIKit rounds a plain button's own background. Fill the Compose canvas cutout behind it.
private class RepliesButtonHost(val button: UIButton) : UIView(frame = platform.CoreGraphics.CGRectMake(0.0, 0.0, 0.0, 0.0)) {
    init {
        button.setFrame(bounds)
        button.autoresizingMask = UIViewAutoresizingFlexibleWidth or UIViewAutoresizingFlexibleHeight
        addSubview(button)
    }
}

@Composable
internal fun AppleRepliesButton(count: Int, onOpen: () -> Unit) {
    val open by rememberUpdatedState(onOpen)
    val surface = MaterialTheme.colorScheme.surface
    val background = remember(surface) {
        UIColor.colorWithRed(surface.red.toDouble(), surface.green.toDouble(), surface.blue.toDouble(), 1.0)
    }
    val configuration = remember(count, LocalDensity.current.fontScale) {
        UIButtonConfiguration.plainButtonConfiguration().apply {
            title = count.toString()
            image = UIImage.systemImageNamed("bubble.left")
            imagePadding = 4.0
            buttonSize = UIButtonConfigurationSize.UIButtonConfigurationSizeSmall
            contentInsets = NSDirectionalEdgeInsetsMake(0.0, 0.0, 0.0, 0.0)
            baseForegroundColor = UIColor.secondaryLabelColor
        }
    }
    var size by remember { mutableStateOf(DpSize(44.dp, 44.dp)) }
    UIKitView(
        factory = {
            RepliesButtonHost(UIButton.buttonWithConfiguration(configuration, UIAction.actionWithHandler { open() }))
        },
        modifier = Modifier.size(size),
        update = { host ->
            host.backgroundColor = background
            val button = host.button
            button.configuration = configuration
            button.accessibilityLabel = if (count == 1) "1 reply" else "$count replies"
            size = button.intrinsicContentSize.useContents {
                DpSize(ceil(width).coerceAtLeast(44.0).dp, ceil(height).coerceAtLeast(44.0).dp)
            }
        },
        properties = UIKitInteropProperties(isNativeAccessibilityEnabled = true),
    )
}
