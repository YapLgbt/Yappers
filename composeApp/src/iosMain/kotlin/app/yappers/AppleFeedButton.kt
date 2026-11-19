@file:OptIn(kotlinx.cinterop.ExperimentalForeignApi::class)
package app.yappers

import androidx.compose.foundation.layout.BoxWithConstraints
import androidx.compose.foundation.layout.size
import androidx.compose.runtime.*
import androidx.compose.ui.ExperimentalComposeUiApi
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.unit.DpSize
import androidx.compose.ui.unit.dp
import androidx.compose.ui.viewinterop.UIKitInteropProperties
import androidx.compose.ui.viewinterop.UIKitView
import kotlin.math.ceil
import kotlinx.cinterop.useContents
import platform.UIKit.UIColor
import platform.UIKit.systemBackgroundColor
import platform.UIKit.UIView
import platform.CoreGraphics.CGSizeMake

interface AppleFeedControl {
    val view: UIView
    fun configureControl(row: FormRow)
}

@OptIn(ExperimentalComposeUiApi::class)
@Composable
internal fun AppleFeedButton(factory: AppleEditorFactory, row: FormRow, modifier: Modifier) {
    val control = remember(factory) { factory.makeFeedControl() }
    val overArtwork = row.kind == "method" || row.kind == "back" || row.kind == "link" || row.kind == "primary" || row.kind == "secondary"
    AppleSizedView(control.view, modifier, overArtwork) { control.configureControl(row) }
}

@Composable
internal fun AppleChatBubble(factory: AppleEditorFactory, bubble: ChatBubble, modifier: Modifier) {
    val control = remember(factory) { factory.makeChatBubble() }
    AppleSizedView(control.view, modifier, false) { control.render(bubble) }
}

interface AppleChatBubbleControl {
    val view: UIView
    fun render(bubble: ChatBubble)
}

@OptIn(ExperimentalComposeUiApi::class)
@Composable
private fun AppleSizedView(view: UIView, modifier: Modifier, overArtwork: Boolean, configure: () -> Unit) {
    val fontScale = LocalDensity.current.fontScale
    BoxWithConstraints(modifier) {
        val width = if (maxWidth.value.isFinite()) maxWidth.value.toDouble() else 760.0
        var size by remember { mutableStateOf(DpSize(44.dp, 44.dp)) }
        key(fontScale) {
            UIKitView(
                factory = { view }, modifier = Modifier.size(size),
                update = {
                    it.backgroundColor = if (overArtwork) UIColor.clearColor else UIColor.systemBackgroundColor
                    configure()
                    size = view.sizeThatFits(CGSizeMake(width, 100000.0)).useContents { DpSize(ceil(this.width).dp, ceil(height).dp) }
                },
                properties = UIKitInteropProperties(isNativeAccessibilityEnabled = true, placedAsOverlay = overArtwork),
            )
        }
    }
}
