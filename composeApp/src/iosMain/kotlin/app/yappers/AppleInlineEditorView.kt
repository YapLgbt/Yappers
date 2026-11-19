@file:OptIn(kotlinx.cinterop.ExperimentalForeignApi::class)

package app.yappers

import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.uikit.LocalUIViewController
import androidx.compose.ui.unit.dp
import androidx.compose.ui.viewinterop.UIKitInteropInteractionMode
import androidx.compose.ui.viewinterop.UIKitInteropProperties
import androidx.compose.ui.viewinterop.UIKitView
import platform.UIKit.UIColor
import platform.UIKit.systemBackgroundColor

@Composable
internal fun AppleInlineEditorView(factory: AppleEditorFactory, model: InlineEditorModel) {
    val controller = LocalUIViewController.current
    var height by remember { mutableStateOf(56.dp) }
    val editor = remember(controller, factory) {
        factory.makeInlineEditor(controller) { height = it.dp }
    }
    UIKitView(
        factory = { editor.view },
        modifier = Modifier.fillMaxWidth().height(height),
        update = { view -> view.backgroundColor = UIColor.systemBackgroundColor; editor.render(model) },
        properties = UIKitInteropProperties(interactionMode = UIKitInteropInteractionMode.NonCooperative, isNativeAccessibilityEnabled = true),
    )
}
